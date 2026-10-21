//! The right button, and every menu it opens: on the thing clicked, never
//! on whatever the cursor happened to be on (the menu matrix, §1.2), the
//! cursor still moving there so the keyboard follows; inside an overlay,
//! on the row clicked, standing on the overlay (`ModalState::verbs_under`);
//! on no row, the background's. The rows of the things the registry has
//! no kind for — a pull request, a branch, a task, an issue, a lane — are
//! built here, one builder for each, so a right-click, `?` and a row's
//! `⋯` all list the same words (docs/superpowers/specs/2026-10-16-kete-
//! 054-menu-matrix.md §5).
//!
//! The terminal app has no headings, rules or disabled rows: a row the
//! desktop greys out is left out here, and a state banner goes in the
//! menu's title (matrix §1.7, §1.8).

use super::verbs::{self, Target};
use super::*;
use crate::app::FileOp;
use crate::keymap::Action;
use crate::palette::PaletteTarget;
use nebula_config::actions::{menu_of, AppliesTo};
use ratatui::layout::Position;

/// What a right-click on `hit` acts on: the card, band, row or tab under
/// the pointer, whatever the cursor is on. None on what has no menu of its
/// own (a button, the footer) — or one of the rows whose menu is built
/// here rather than from the registry (a pull request's, an issue's, a
/// link's).
pub(crate) fn hit_to_target(app: &App, hit: &HitTarget) -> Option<Target> {
    let card = |sref: SessionRef| match sref {
        SessionRef::Agent(id) => Target::Session(id),
        SessionRef::Terminal(id) => Target::Terminal(id),
    };
    // A band on the grid is its checkout, unless it is a NESTED thread of
    // the root, whose one card is what the band stands for.
    let band = |i: usize| {
        let bands = crate::launcher::bands(app);
        let b = bands.get(i)?;
        if b.solo {
            b.cards.first().map(|c| card(c.sref()))
        } else {
            Some(Target::Worktree(b.worktree.clone()))
        }
    };
    match hit {
        HitTarget::ProjectRow(i) => app
            .project_rows()
            .get(*i)
            .and_then(|p| app.tree.projects.get(*p))
            .map(|p| Target::Repo(p.id.clone())),
        HitTarget::WorktreeRow(i) => app
            .worktree_rows()
            .get(*i)
            .and_then(|r| r.checkout())
            .map(|w| Target::Worktree(w.id.clone())),
        HitTarget::SessionRow(i) => match app.visible_session_rows().get(*i)? {
            SessionRow::Agent(a) => Some(Target::Session(a.id.clone())),
            SessionRow::Terminal(t) => Some(Target::Terminal(t.id.clone())),
            SessionRow::Link(_) => None,
        },
        HitTarget::LauncherCard(at) | HitTarget::CardButton(at, _) | HitTarget::CardMenu(at) => {
            let bands = crate::launcher::bands(app);
            crate::launcher::card_at(&bands, *at).map(|c| card(c.sref()))
        }
        HitTarget::KanbanCard(id)
        | HitTarget::KanbanButton(id, _)
        | HitTarget::KanbanMenu(id)
        | HitTarget::LauncherCardIssue(id)
        | HitTarget::SubagentChip(id) => Some(Target::Session(id.clone())),
        HitTarget::OrchestratorChip(sref) => Some(card(sref.clone())),
        HitTarget::LauncherBand(i)
        | HitTarget::LauncherBandMore(i)
        | HitTarget::LauncherBandFold(i)
        | HitTarget::LauncherBandNew(i) => band(*i),
        HitTarget::LauncherTab(p) => Some(Target::Repo(p.clone())),
        HitTarget::PanelBg(_) => Some(Target::Global),
        _ => None,
    }
}

/// Open `items` at `at`, titled (a state banner) or not: one of the grid's
/// own menus, nothing waiting under it.
fn open_at(app: &mut App, title: Option<String>, items: Vec<MenuItem>, at: (u16, u16)) {
    open_menu(app, items, at);
    if let Some(Overlay::Menu(menu)) = &mut app.modals.overlay {
        menu.title = title;
    }
}

/// `t`'s menu hung at `at`: its rows, folded, and its banner as the title.
pub(crate) fn open_target_menu(app: &mut App, t: &Target, at: (u16, u16)) {
    let items = match t {
        Target::Global => background_items(app, None),
        _ => verbs::menu_for(app, t),
    };
    let title = verbs::menu_title(app, t);
    open_at(app, title, items, at);
}

/// The right button on the grid or the panels: the cursor onto the thing
/// under the pointer as a left click puts it (`select_clicked_row`), then
/// that thing's menu at the pointer — built from what was clicked, so a
/// band's rule gives its checkout's menu and a card under a header that
/// holds the keys gives the card's.
pub(crate) fn right_click(app: &mut App, mouse: MouseEvent, out: &mut Vec<ClientRequest>) {
    let at = (mouse.column, mouse.row);
    let Some(hit) = app.hit_at(mouse.column, mouse.row) else {
        return;
    };
    // The click names its own thing: the header's tab cursor lets the
    // keys go back to the cards, as a left click does.
    if !matches!(hit, HitTarget::LauncherTab(_) | HitTarget::LauncherTabMore) {
        super::launcher::leave_tabs(app);
    }
    match hit {
        // A PROJECT TAB has no cursor to move, but it is the project's
        // own handle: the right button opens it, with the project's menu
        // hung under the tab.
        HitTarget::LauncherTab(id) => super::launcher::tab_menu(app, &id, out),
        // The MORE CHIP has no menu but its list: either button drops it.
        HitTarget::LauncherTabMore => super::launcher::open_more_tabs_menu(app),
        // A panel's or the grid's background: the panel's own rows (the
        // columns'), then the app's.
        HitTarget::PanelBg(focus) => {
            app.nav.focus = focus;
            let items = background_items(app, Some(focus));
            open_at(app, None, items, at);
        }
        // A board lane's air: what the lane takes in bulk, then the
        // background's.
        HitTarget::KanbanLane(lane) => {
            app.nav.focus = Focus::Sessions;
            let (title, items) = lane_items(app, lane);
            open_at(app, Some(title), items, at);
        }
        // An attention count: the sessions it counts.
        HitTarget::Attention(tone) => {
            let (title, items) = attention_items(app, tone);
            open_at(app, Some(title), items, at);
        }
        // The pane's account chip keeps the account's own menu, as its
        // left click opens it (matrix §3.3).
        HitTarget::AccountChip(id) => crate::account_switch::open_picker(app, id),
        // A pull request on a band's rule or a thread's column: the pull
        // request's menu, the band under the cursor.
        HitTarget::LauncherBandPr(w) | HitTarget::LauncherThreadPr(w) => {
            select_clicked_row(app, &HitTarget::LauncherBandPr(w.clone()), out);
            if let Some(pr) = crate::launcher::worktree_pr(app, &w) {
                let items = band_pr_items(app, &w, &pr);
                open_at(app, Some(format!("pull request #{}", pr.number)), items, at);
            }
        }
        hit => {
            let target = hit_to_target(app, &hit);
            // A panel's row: that panel takes the keys, as the left
            // button's click gives them.
            match hit {
                HitTarget::ProjectRow(_) => app.nav.focus = Focus::Projects,
                HitTarget::WorktreeRow(_) => app.nav.focus = Focus::Worktrees,
                HitTarget::SessionRow(_) => app.nav.focus = Focus::Sessions,
                _ => {}
            }
            let moved = select_clicked_row(app, &hit, out);
            match target {
                // A project's row in the Projects column keeps the
                // column's own Add project… after the project's rows.
                Some(t) if matches!(hit, HitTarget::ProjectRow(_)) => {
                    let mut items = verbs::menu_for(app, &t);
                    items.extend(panel_menu_items(app, Focus::Projects));
                    open_at(app, verbs::menu_title(app, &t), items, at);
                }
                Some(t) => open_target_menu(app, &t, at),
                // A pull request's, an issue's or a link's row in the
                // panels: its own menu, the row under the cursor.
                None if moved => {
                    if let Some((title, items)) = selected_row_menu(app) {
                        open_at(app, Some(title), items, at);
                    }
                }
                None => {}
            }
        }
    }
    app.chrome.dirty = true;
}

/// A right-click on an overlay's row: the cursor there as a left click
/// puts it, then that row's menu standing on the overlay — a diff's file,
/// the tree's file or folder, a finder's or a search's hit, an account,
/// an Azure DevOps task, pull request or branch, a pull request's or an
/// issue's row. False where the pointer is on no row, so the overlay's
/// own mouse handling takes it.
pub(crate) fn overlay_right_click(app: &mut App, pos: Position) -> bool {
    let menu = match &app.modals.overlay {
        Some(Overlay::Diff(_)) => diff_row_menu(app, Some(pos)),
        Some(Overlay::Tree(_)) => tree_row_menu(app, pos),
        Some(Overlay::Files(_)) => files_row_menu(app, pos),
        Some(Overlay::Grep(_)) => grep_row_menu(app, pos),
        Some(Overlay::Accounts(v)) if v.tab == crate::accounts_overlay::Tab::Wake => {
            wake_row_menu(app, Some(pos))
        }
        Some(Overlay::Accounts(_)) => accounts_row_menu(app, Some(pos)),
        Some(Overlay::Ado(_)) => ado_row_menu(app, Some(pos)),
        Some(Overlay::PullRequests(_)) => pr_modal_row_menu(app, pos),
        Some(Overlay::Issues(_)) => issue_modal_row_menu(app, pos),
        _ => None,
    };
    let Some((title, items)) = menu else {
        return false;
    };
    if !items.is_empty() {
        open_over(app, Some(title), items, Some((pos.x, pos.y)), false);
    }
    app.chrome.dirty = true;
    true
}

/// `?` over an overlay with rows of its own (the diff's file list, the
/// accounts', Azure DevOps'): the row's menu, then the overlay's own keys,
/// the overlay kept beneath. False where `?` is the overlay's own (a
/// filter being typed, a form, a reply).
pub(crate) fn open_over_overlay(app: &mut App) -> bool {
    let menu = match &app.modals.overlay {
        Some(Overlay::Diff(v)) if v.filter.is_empty() && v.confirm_discard.is_none() => {
            let (title, mut items) = diff_row_menu(app, None).unwrap_or_else(|| {
                let file = match &app.modals.overlay {
                    Some(Overlay::Diff(v)) => v.selected_path().unwrap_or_default().to_string(),
                    _ => String::new(),
                };
                (format!("diff {file}"), Vec::new())
            });
            items.extend(keys_rows(DIFF_KEYS));
            Some((title, items))
        }
        // The Wake view: its row's keys, which are not the list's (lane/wake).
        Some(Overlay::Accounts(v))
            if matches!(v.mode, crate::accounts_overlay::Mode::List)
                && v.tab == crate::accounts_overlay::Tab::Wake =>
        {
            let (title, mut items) = wake_row_menu(app, None)
                .unwrap_or_else(|| ("wake-up".into(), keys_rows(WAKE_KEYS)));
            items.extend(keys_rows(&[CLOSE_KEY]));
            Some((title, items))
        }
        Some(Overlay::Accounts(v)) if matches!(v.mode, crate::accounts_overlay::Mode::List) => {
            let (title, mut items) =
                accounts_row_menu(app, None).unwrap_or_else(|| ("accounts".into(), Vec::new()));
            items.extend(keys_rows(ACCOUNT_KEYS));
            Some((title, items))
        }
        Some(Overlay::Ado(v)) if v.voting.is_none() && v.threads.is_none() => {
            let (title, mut items) =
                ado_row_menu(app, None).unwrap_or_else(|| ("Azure DevOps".into(), Vec::new()));
            items.extend(keys_rows(ADO_KEYS));
            Some((title, items))
        }
        _ => None,
    };
    let Some((title, items)) = menu else {
        return false;
    };
    open_over(app, Some(title), items, None, true);
    true
}

/// Put a menu up over the overlay that is up, which waits under it: drawn
/// beneath the menu, back when the menu goes, back too after a row that
/// leaves the user where they were (a copy, a stage, a key of its own).
/// `typed`: the verbs menu's type-ahead and **All keys…** (`?`).
fn open_over(
    app: &mut App,
    title: Option<String>,
    items: Vec<MenuItem>,
    at: Option<(u16, u16)>,
    typed: bool,
) {
    let under = app.modals.overlay.take().map(Box::new);
    let menu = if typed {
        verbs::verbs_menu(title.unwrap_or_default(), items)
    } else {
        ContextMenu {
            title,
            items,
            at,
            hover: 0,
            area: ratatui::layout::Rect::default(),
            scroll: 0,
            parent: None,
            filter: None,
        }
    };
    app.modals.overlay = Some(Overlay::Menu(menu));
    app.modals.verbs_under = under;
    app.chrome.dirty = true;
}

/// Is `action` a row that leaves the user where they were, so the overlay
/// the menu stood on comes back after it: a copy, a stage, an editor or a
/// browser opened beside Kete, a key the overlay answers itself. A row
/// that goes somewhere (a session, a band, a box) takes the overlay away.
pub(crate) fn keeps_overlay(action: &MenuAction) -> bool {
    match action {
        MenuAction::Copy(_)
        | MenuAction::OpenLink(_)
        // A theme picked: the settings it was picked from come back.
        | MenuAction::PickTheme(_)
        | MenuAction::OverlayKey(..)
        | MenuAction::OverlayKeys(_)
        | MenuAction::FileGit { .. }
        | MenuAction::FoldTree(_) => true,
        // An editor, Explorer or the default app opened beside Kete; not
        // Kete's own file tabs, which take the screen.
        MenuAction::Editor(act) => !matches!(act, crate::editors::EditorAct::View { .. }),
        MenuAction::Verb { action, .. } => matches!(
            action,
            Action::OpenInVscode
                | Action::OpenInVisualStudio
                | Action::RevealInExplorer
                | Action::OpenWithDefault
                | Action::DockEditor
                | Action::WakeAccount
                // The accounts overlay turned to its Wake view, not a new one.
                | Action::WakeSchedule
        ),
        _ => false,
    }
}

// ---- the background, a lane, the attention strip ----

/// The menu of no row: in the columns, the panel's own rows first (Add
/// project, New worktree, the open-PRs folds, New agent, Show archived);
/// then the app's (matrix §5.15), less a verb the panel rows already
/// carry.
pub(crate) fn background_items(app: &App, panel: Option<Focus>) -> Vec<MenuItem> {
    let columns = !app.launcher_active() || app.launcher.launcher_columns;
    let mut items = match panel {
        Some(focus) if columns => panel_menu_items(app, focus),
        _ => Vec::new(),
    };
    let have: Vec<MenuAction> = items.iter().map(|i| i.action.clone()).collect();
    let taken = |i: &MenuItem| have.contains(&i.action);
    for item in verbs::menu_for(app, &Target::Global) {
        match item.action {
            MenuAction::Submenu { title, items: rows } => {
                let rows: Vec<MenuItem> = rows.into_iter().filter(|r| !taken(r)).collect();
                match rows.len() {
                    0 => {}
                    1 => items.extend(rows),
                    _ => items.push(verbs::submenu(title, rows)),
                }
            }
            _ if taken(&item) => {}
            _ => items.push(item),
        }
    }
    items
}

/// A board lane's own rows (matrix §5.16) — the next card waiting on you
/// or running, every unread one marked seen, the pull requests read
/// again, the done ones archived — then the background's.
fn lane_items(app: &App, lane: usize) -> (String, Vec<MenuItem>) {
    use crate::launcher::Lane;
    let Some(lane) = Lane::ALL.get(lane).copied() else {
        return ("Board".into(), background_items(app, None));
    };
    let global = Target::Global;
    let mut items: Vec<MenuItem> = match lane {
        Lane::NeedsInput => vec![MenuItem::new(
            "Open next needing you",
            MenuAction::Jump(PaletteTarget::Attention(crate::subagents::Tone::Needs)),
        )
        .keyed(Action::NextAttention)],
        Lane::Running => vec![MenuItem::new(
            "Open next running",
            MenuAction::Jump(PaletteTarget::Attention(crate::subagents::Tone::Running)),
        )],
        Lane::Review => verbs::rows(app, &["mark_seen"], &global),
        Lane::Ship => vec![MenuItem::new(
            "Refresh pull requests",
            MenuAction::Run(Action::RefreshPullRequests),
        )
        .keyed(Action::RefreshPullRequests)],
        Lane::Done => verbs::rows(app, &["archive_done"], &global),
    };
    let have: Vec<MenuAction> = items.iter().map(|i| i.action.clone()).collect();
    items.extend(
        background_items(app, None)
            .into_iter()
            .filter(|i| !have.contains(&i.action)),
    );
    (lane.title().to_string(), items)
}

/// An attention count's list (matrix §5.19): the sessions it counts,
/// newest change first, each opening its session; the unread count ends
/// on Mark all seen.
fn attention_items(app: &App, tone: crate::subagents::Tone) -> (String, Vec<MenuItem>) {
    let ids = crate::attention::tally(app)
        .remove(&tone)
        .unwrap_or_default();
    let mut agents: Vec<&nebula_core::Agent> = ids
        .iter()
        .filter_map(|id| app.tree.agents.iter().find(|a| &a.id == id))
        .collect();
    agents.sort_by_key(|a| std::cmp::Reverse(a.status_changed_at));
    let mut items: Vec<MenuItem> = agents
        .iter()
        .map(|a| {
            MenuItem::new(
                a.name.clone(),
                MenuAction::Jump(PaletteTarget::Session(a.id.clone())),
            )
        })
        .collect();
    if tone == crate::subagents::Tone::Unread {
        items.extend(verbs::rows(app, &["mark_seen"], &Target::Global));
    }
    let title = match tone {
        crate::subagents::Tone::Needs => "needs you".to_string(),
        other => other.word().to_string(),
    };
    (title, items)
}

// ---- the panels' rows the registry has no kind for ----

/// The menu of the row under the cursor when it is no registry thing: a
/// pull request's or an issue's row in the Worktrees column, a link's in
/// the Sessions column — what a right-click on it, and `?` on it, list.
pub(crate) fn selected_row_menu(app: &App) -> Option<(String, Vec<MenuItem>)> {
    match app.nav.focus {
        Focus::Worktrees => {
            if let Some(pr) = app.selected_worktree_pr() {
                return Some((
                    format!("pull request #{}", pr.number),
                    pr_row_menu_items(app, pr),
                ));
            }
            let issue = app.selected_worktree_issue()?;
            Some((
                format!("issue #{}", issue.number),
                issue_row_menu_items(issue),
            ))
        }
        _ => {
            let SessionRow::Link(link) = app.selected_session_row()? else {
                return None;
            };
            Some((format!("link {}", link.label()), menu_items_for_link(&link)))
        }
    }
}

/// The checkout on `branch` in `project`, when one is.
fn worktree_on<'a>(
    app: &'a App,
    project: &ProjectId,
    branch: &str,
) -> Option<&'a nebula_core::Worktree> {
    app.tree
        .worktrees
        .iter()
        .find(|w| &w.project_id == project && w.branch == branch)
}

/// The live session in checkout `w` that moved last, for **Go to session**.
fn session_in(app: &App, w: &WorktreeId) -> Option<nebula_core::Agent> {
    app.tree
        .agents
        .iter()
        .filter(|a| &a.worktree_id == w && !a.archived)
        .max_by_key(|a| a.status_changed_at)
        .cloned()
}

/// **Go to session ‹name›** and **Go to worktree ‹branch›** for work on
/// `branch`: the connections from a pull request or a branch to where it
/// is being done (matrix §6). `skip_worktree`: the menu was opened on
/// that worktree's own band.
fn go_rows(
    app: &App,
    project: &ProjectId,
    branch: &str,
    skip_worktree: bool,
) -> (Vec<MenuItem>, bool) {
    let mut rows = Vec::new();
    let Some(w) = worktree_on(app, project, branch) else {
        return (rows, false);
    };
    if let Some(a) = session_in(app, &w.id) {
        rows.push(MenuItem::new(
            format!("Go to session {}", a.name),
            MenuAction::Jump(PaletteTarget::Session(a.id)),
        ));
    }
    if !skip_worktree {
        rows.push(MenuItem::new(
            format!("Go to worktree {}", w.branch),
            MenuAction::Jump(PaletteTarget::Worktree(w.id.clone())),
        ));
    }
    (rows, true)
}

/// `Copy ▸` over `rows` (word, text), folded once there are two.
fn copy_rows(rows: Vec<(String, String)>) -> Vec<MenuItem> {
    let items: Vec<MenuItem> = rows
        .into_iter()
        .map(|(word, text)| MenuItem::new(word, MenuAction::Copy(text)))
        .collect();
    match items.len() {
        0 | 1 => items,
        _ => vec![verbs::submenu("Copy", items)],
    }
}

/// The Copy rows' words standing alone, `Copy link`, or inside `Copy ▸`.
fn copies(rows: &[(&str, String)]) -> Vec<MenuItem> {
    if rows.len() == 1 {
        let (word, text) = &rows[0];
        return copy_rows(vec![(
            format!("Copy {}", word.to_lowercase()),
            text.clone(),
        )]);
    }
    copy_rows(
        rows.iter()
            .map(|(w, t)| (w.to_string(), t.clone()))
            .collect(),
    )
}

/// The CONTEXT MENU for a PROJECT OPEN PRS GROUP row (matrix §5.10): the
/// browser; a PR SESSION on it, one row per enabled harness under **New
/// session on this pull request ▸** (none when the PROJECT has no ROOT
/// WORKTREE to launch in); where its work is; its diff; its link, number
/// and branch; a comment; the merge, from the checkout on its branch;
/// the lists read again.
pub(crate) fn pr_row_menu_items(app: &App, pr: &crate::pull_request::OpenPr) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::new("Open in browser", MenuAction::OpenLink(pr.url.clone()))
            .keyed(Action::Activate),
    ];
    if let Some(worktree) = selected_project_main_worktree(app) {
        let sessions = crate::agent_picker::pr_session_menu_rows(worktree, pr);
        if !sessions.is_empty() {
            items.push(verbs::submenu("New session on this pull request", sessions));
        }
    }
    let project = app.selected_project().map(|p| p.id.clone());
    let mut checkout = None;
    if let Some(project) = &project {
        let (go, on) = go_rows(app, project, &pr.head, false);
        items.extend(go);
        if on {
            checkout = worktree_on(app, project, &pr.head).map(|w| w.id.clone());
        }
    }
    items.push(MenuItem::new("View diff", MenuAction::ViewPrDiff).keyed(Action::GitDiff));
    items.extend(copies(&[
        ("Link", pr.url.clone()),
        ("Number", format!("#{}", pr.number)),
        ("Branch name", pr.head.clone()),
    ]));
    items.push(
        MenuItem::new("Comment\u{2026}", MenuAction::CommentPullRequest)
            .keyed(Action::CommentPullRequest),
    );
    if let (Some(worktree), false) = (checkout, pr.is_draft) {
        items.push(MenuItem::new(
            "Merge\u{2026}",
            MenuAction::MergePullRequest {
                worktree,
                agent: None,
            },
        ));
    }
    items.push(
        MenuItem::new(
            "Refresh pull requests",
            MenuAction::Run(Action::RefreshPullRequests),
        )
        .keyed(Action::RefreshPullRequests),
    );
    items
}

/// The menu of a checkout's pull request on its band's rule (or a thread's
/// `#42`): the browser, the session at work on it, its link, number and
/// branch, a comment and the merge while it is open, the lists read
/// again. Merged or closed, only what reads it (matrix §5.10).
fn band_pr_items(app: &App, w: &WorktreeId, pr: &crate::launcher::RowPr) -> Vec<MenuItem> {
    use crate::pull_request::Standing;
    let branch = app
        .tree
        .worktrees
        .iter()
        .find(|x| &x.id == w)
        .map(|x| x.branch.clone())
        .unwrap_or_default();
    let mut items = vec![
        MenuItem::new("Open in browser", MenuAction::OpenLink(pr.url.clone()))
            .keyed(Action::OpenPullRequest),
    ];
    if let Some(a) = session_in(app, w) {
        items.push(MenuItem::new(
            format!("Go to session {}", a.name),
            MenuAction::Jump(PaletteTarget::Session(a.id)),
        ));
    }
    items.extend(copies(&[
        ("Link", pr.url.clone()),
        ("Number", format!("#{}", pr.number)),
        ("Branch name", branch),
    ]));
    if matches!(pr.standing, Standing::Open | Standing::Draft) {
        items.extend(verbs::rows(
            app,
            &["comment_pull_request", "merge_pull_request"],
            &Target::Worktree(w.clone()),
        ));
        items.push(
            MenuItem::new(
                "Refresh pull requests",
                MenuAction::Run(Action::RefreshPullRequests),
            )
            .keyed(Action::RefreshPullRequests),
        );
    }
    items
}

/// The CONTEXT MENU for a PROJECT ISSUES GROUP row (matrix §5.24): the
/// browser, then a session on it — the QUICK PROMPT on the issue (`p`) or
/// an AGENT PRESET (`e`) — and its link and number.
pub(crate) fn issue_row_menu_items(issue: &crate::issues::Issue) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::new("Open in browser", MenuAction::OpenLink(issue.url.clone()))
            .keyed(Action::Activate),
        verbs::submenu(
            "New session on this issue",
            vec![
                MenuItem::new("Quick prompt\u{2026}", MenuAction::Run(Action::QuickPrompt))
                    .keyed(Action::QuickPrompt),
                MenuItem::new(
                    "From an agent preset\u{2026}",
                    MenuAction::Run(Action::AgentPresets),
                )
                .keyed(Action::AgentPresets),
            ],
        ),
    ];
    items.extend(copies(&[
        ("Link", issue.url.clone()),
        ("Number", format!("#{}", issue.number)),
    ]));
    items
}

// ---- files and folders ----

/// What a file row knows of its file, for the rows its state rules in or
/// out (matrix §5.8).
#[derive(Default)]
struct FileState {
    /// git's two status letters, where the row has them (a diff's).
    xy: Option<[char; 2]>,
    /// The diff is read against HEAD in a checkout of its own, so a file
    /// stages, unstages and discards here.
    stages: bool,
    /// The diff's review marks: Some(marked) on a diff's row.
    reviewed: Option<bool>,
    /// A pull request's prefetched diff: no checkout of its own on disk.
    remote_only: bool,
    /// Found from the tree or a finder in a checkout with changes (or not
    /// yet counted): **Diff this file** opens the diff on it.
    diffable: bool,
}

/// What a file row found from the tree or a finder knows: no git status
/// of its own, but whether its checkout (the one under the cursor, the
/// one the overlay lists) has changes to diff — unknown counts as yes.
fn found(app: &App, root: &std::path::Path) -> FileState {
    let diffable = app
        .selected_worktree()
        .is_some_and(|w| w.path == root && app.worktree_changes(&w.id) != Some(0));
    FileState {
        diffable,
        ..FileState::default()
    }
}

/// A file's menu: the registry's rows less what its state rules out (a
/// deleted file opens nowhere; a pull request's file is no path here),
/// then the diff's own: Stage or Unstage, Mark reviewed, Discard changes.
fn file_items(app: &App, root: &std::path::Path, rel: &str, state: &FileState) -> Vec<MenuItem> {
    let t = Target::File {
        worktree: root.to_path_buf(),
        rel: rel.to_string(),
    };
    let deleted = state.xy.is_some_and(|xy| xy.contains(&'D'));
    let opens = |v: Option<Action>| {
        matches!(
            v,
            Some(
                Action::OpenFileInKete
                    | Action::OpenInVscode
                    | Action::OpenInVisualStudio
                    | Action::OpenWithDefault
                    | Action::RevealInExplorer
            )
        )
    };
    let mut items: Vec<MenuItem> = verbs::rows(app, menu_of(AppliesTo::File), &t)
        .into_iter()
        .filter(|i| !(deleted && opens(i.verb)))
        .filter(|i| !state.remote_only || i.verb == Some(Action::CopyRelativePath))
        .collect();
    if state.diffable {
        // Beside Open in Kete, ahead of the other apps.
        let at = usize::from(items.first().and_then(|i| i.verb) == Some(Action::OpenFileInKete));
        items.insert(
            at,
            MenuItem::new("Diff this file", MenuAction::DiffThisFile(rel.to_string())),
        );
    }
    let file_git = |label: &str, op: FileOp| {
        let item = MenuItem::new(
            label,
            MenuAction::FileGit {
                root: root.to_path_buf(),
                rel: rel.to_string(),
                op,
            },
        );
        if op == FileOp::Discard {
            MenuItem {
                destructive: true,
                ..item
            }
        } else {
            item
        }
    };
    let xy = state.xy.unwrap_or([' ', ' ']);
    let untracked = xy == ['?', '?'];
    if state.stages {
        if xy[1] != ' ' {
            items.push(file_git("Stage file", FileOp::Stage));
        }
        if !untracked && xy[0] != ' ' {
            items.push(file_git("Unstage file", FileOp::Unstage));
        }
    }
    if let Some(marked) = state.reviewed {
        let label = if marked {
            "Mark reviewed \u{2713}"
        } else {
            "Mark reviewed"
        };
        items.push(verbs::overlay_key_row(
            label,
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
            false,
        ));
    }
    if state.stages && !untracked && xy[1] != ' ' {
        items.push(file_git("Discard changes\u{2026}", FileOp::Discard));
    }
    items
}

/// A folder's menu: the registry's rows (an editor, Explorer, its paths).
fn folder_items(app: &App, root: &std::path::Path, rel: &str) -> Vec<MenuItem> {
    let t = Target::Folder {
        worktree: root.to_path_buf(),
        rel: rel.to_string(),
    };
    verbs::rows(app, menu_of(AppliesTo::Folder), &t)
}

/// The diff's row under `pos` (None: the cursor's own, for `?`): the
/// cursor onto it as a click puts it, never folding a directory, and its
/// file's or folder's menu.
fn diff_row_menu(app: &mut App, pos: Option<Position>) -> Option<(String, Vec<MenuItem>)> {
    let Some(Overlay::Diff(view)) = &mut app.modals.overlay else {
        return None;
    };
    if let Some(pos) = pos {
        let area = view.list_area;
        let index = if view.log.is_some() {
            let first = crate::app::window_start(view.side_cursor(), area.height as usize);
            crate::list_hit::row_at(area, first, view.side_len(), pos).map(|i| i as i64)
        } else {
            let first = view.window_start(area.height as usize);
            crate::list_hit::row_at(area, first, view.row_count(), pos).map(|i| i as i64 + 1)
        }?;
        activate::diff_file(view, index);
    }
    let view = match &app.modals.overlay {
        Some(Overlay::Diff(view)) => view,
        _ => return None,
    };
    let root = view.root.clone();
    if let Some(file) = view.selected_file() {
        let state = FileState {
            xy: Some(file.xy),
            stages: view.prefetched.is_none() && view.diff_base().is_none(),
            reviewed: Some(view.reviewed.contains_key(&file.path)),
            remote_only: view.prefetched.is_some(),
            // Already its diff.
            diffable: false,
        };
        let rel = file.path.clone();
        return Some((format!("diff {rel}"), file_items(app, &root, &rel, &state)));
    }
    let dir = view.selected_dir()?.to_string();
    let items = if view.prefetched.is_some() {
        Vec::new()
    } else {
        folder_items(app, &root, &dir)
    };
    Some((format!("diff {dir}"), items))
}

/// The tree browser's row under `pos`: a folder's menu, or a file's.
fn tree_row_menu(app: &mut App, pos: Position) -> Option<(String, Vec<MenuItem>)> {
    let Some(Overlay::Tree(view)) = &mut app.modals.overlay else {
        return None;
    };
    let index = view.list.hit(pos)?;
    view.select(index as i64);
    let node = view.selected_node()?.clone();
    let root = view.root.clone();
    // A folder's own: open it all, or fold it all, while the tree is
    // not filtered (a filter opens every folder it keeps).
    let folds = node.is_dir && !node.children.is_empty() && view.list.query.is_empty();
    let items = if node.is_dir {
        let mut items = folder_items(app, &root, &node.path);
        if folds {
            items.push(MenuItem::new("Expand all", MenuAction::FoldTree(true)));
            items.push(MenuItem::new("Collapse all", MenuAction::FoldTree(false)));
        }
        items
    } else {
        file_items(app, &root, &node.path, &found(app, &root))
    };
    Some((node.path, items))
}

/// The file finder's hit under `pos`.
fn files_row_menu(app: &mut App, pos: Position) -> Option<(String, Vec<MenuItem>)> {
    let Some(Overlay::Files(finder)) = &mut app.modals.overlay else {
        return None;
    };
    let row = finder.list.hit(pos)?;
    finder.list.select(row as i64);
    let rel = finder.list.selected()?.clone();
    let root = finder.root.clone();
    let items = file_items(app, &root, &rel, &found(app, &root));
    Some((rel, items))
}

/// Find in files' hit under `pos`: its file's menu.
fn grep_row_menu(app: &mut App, pos: Position) -> Option<(String, Vec<MenuItem>)> {
    let Some(Overlay::Grep(view)) = &mut app.modals.overlay else {
        return None;
    };
    let row = view.list.hit(pos)?;
    view.list.select(row as i64);
    let hit = view.list.selected()?.clone();
    let root = view.root.clone();
    let items = file_items(app, &root, &hit.path, &found(app, &root));
    Some((format!("{}:{}", hit.path, hit.line), items))
}

// ---- accounts ----

/// The keys a key row presses, in an overlay.
fn key(code: char) -> (KeyCode, KeyModifiers) {
    (KeyCode::Char(code), KeyModifiers::NONE)
}

/// An account's menu (matrix §5.9), from what its row says now: its
/// sessions, a login to renew and its command, its switches, wake-up, the
/// session under the grid's cursor moved onto it, its own edits, its
/// removal. In the accounts overlay, so not **Account details…**; no Use
/// for… with no repo to use it for (`repos`), no Rename… while sessions
/// on it block one (`renames`).
fn account_items(
    app: &App,
    row: &crate::accounts_overlay::AccountRow,
    repos: bool,
    renames: bool,
) -> Vec<MenuItem> {
    let t = Target::Account(row.key.clone());
    let profile = row.profile.is_some() && row.problem.is_none();
    let press =
        |label: &str, k: (KeyCode, KeyModifiers)| verbs::overlay_key_row(label, k.0, k.1, false);
    let mut items = Vec::new();
    if !row.sessions.is_empty() {
        items.extend(verbs::rows(app, &["scope_account"], &t));
    }
    let needs_login =
        row.problem.is_none() && row.login == nebula_core::accounts::LoginState::NeedsLogin;
    if needs_login {
        items.push(press("Log in again\u{2026}", key('l')));
        if let Some(cmd) = nebula_core::accounts::login_command(row.profile.as_ref(), &row.harness)
        {
            items.push(MenuItem::new("Copy sign-in command", MenuAction::Copy(cmd)));
        }
    }
    if profile {
        let checked = |on: bool, label: &str| {
            if on {
                format!("{label} \u{2713}")
            } else {
                label.to_string()
            }
        };
        items.push(press(&checked(row.default, "Make default"), key('d')));
        if repos {
            items.push(press("Use for\u{2026}", key('p')));
        }
        items.push(press(&checked(!row.disabled(), "Switch on"), key(' ')));
        items.push(press(
            if row.reserved() {
                "Stop reserving"
            } else {
                "Reserve for me"
            },
            key('y'),
        ));
    }
    // Wake-up runs Claude accounts. Wake up now waits for a login and for
    // the switch (the desktop greys it with why, matrix §5.9); the
    // schedule is always there to read.
    if row.harness == "claude" && row.problem.is_none() {
        let wakes = !needs_login && !row.disabled();
        let ids: &[&str] = if wakes {
            &["wake_account", "wake_schedule"]
        } else {
            &["wake_schedule"]
        };
        items.extend(verbs::rows(app, ids, &t));
    }
    // The session under the grid's cursor, onto this account: same
    // harness, another account.
    if let Some(SessionRow::Agent(a)) = app.selected_session_row() {
        if !a.archived
            && nebula_core::accounts::session_harness(&a) == row.harness
            && nebula_core::accounts::session_account(&a) != row.key
        {
            items.push(press(&format!("Move {} here", a.name), key('m')));
        }
    }
    if row.profile.is_some() {
        if row.problem.is_none() {
            items.push(press("Edit\u{2026}", (KeyCode::Enter, KeyModifiers::NONE)));
        }
        if renames {
            items.push(press("Rename\u{2026}", key('r')));
        }
        items.push(press("Duplicate\u{2026}", key('c')));
        items.push(verbs::overlay_key_row(
            "Remove\u{2026}",
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            true,
        ));
    }
    items
}

/// What an account's menu title says after its name: logged out,
/// switched off, at or near its limit (matrix §4).
fn account_banner(row: &crate::accounts_overlay::AccountRow) -> Option<String> {
    let mut words = Vec::new();
    if row.problem.is_some() {
        words.push("needs fixing".to_string());
    } else if row.login == nebula_core::accounts::LoginState::NeedsLogin {
        words.push(
            if row.expired {
                "login expired"
            } else {
                "logged out"
            }
            .to_string(),
        );
    }
    if row.disabled() {
        words.push("switched off".into());
    }
    if let Some(usage) = &row.usage {
        let five = usage.window(nebula_core::usage::WindowKind::Session);
        if let Some(w) = five.filter(|w| w.percent >= 80.0) {
            words.push(if w.percent >= 100.0 {
                "limited".to_string()
            } else {
                format!("{:.0}% of 5h", w.percent)
            });
        }
    }
    (!words.is_empty()).then(|| words.join(" \u{b7} "))
}

/// The accounts' row under `pos` (None: the cursor's, for `?`).
fn accounts_row_menu(app: &mut App, pos: Option<Position>) -> Option<(String, Vec<MenuItem>)> {
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return None;
    };
    if !matches!(view.mode, crate::accounts_overlay::Mode::List) {
        return None;
    }
    if let Some(pos) = pos {
        let row = view
            .hits
            .iter()
            .find(|(r, _)| r.contains(pos))
            .and_then(|(_, h)| match h {
                crate::accounts_overlay::AccountsHit::Row(i) => Some(*i),
                _ => None,
            })?;
        view.cursor = row.min(view.rows.len().saturating_sub(1));
    }
    let row = view.selected()?.clone();
    let repos = !view.projects.is_empty();
    let blocked = row.profile.as_ref().is_some_and(|p| {
        !view.sessions_follow
            && nebula_core::profiles::rename_blocker(&app.tree.agents, &p.name).is_some()
    });
    let title = match account_banner(&row) {
        Some(banner) => format!("account {} \u{b7} {banner}", row.label()),
        None => format!("account {}", row.label()),
    };
    Some((title, account_items(app, &row, repos, !blocked)))
}

/// The Wake view's row under `pos` (None: the cursor's, for `?`), the
/// cursor moved there: its own keys (matrix §5.21), Accounts last, which
/// turns back to the list on the same account.
fn wake_row_menu(app: &mut App, pos: Option<Position>) -> Option<(String, Vec<MenuItem>)> {
    use crate::accounts_overlay::{AccountsHit, Mode, Tab};
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return None;
    };
    if !matches!(view.mode, Mode::List) || view.tab != Tab::Wake {
        return None;
    }
    if let Some(pos) = pos {
        let row = view
            .hits
            .iter()
            .find(|(r, _)| r.contains(pos))
            .and_then(|(_, h)| match h {
                AccountsHit::WakeRow(i)
                | AccountsHit::WakeSwitch(i)
                | AccountsHit::WakeAnchor(i, _) => Some(*i),
                _ => None,
            })?;
        view.wake.select(row);
    }
    let name = view.wake.selected_key().unwrap_or_default();
    Some((format!("wake-up {name}"), keys_rows(WAKE_KEYS)))
}

// ---- Azure DevOps ----

/// The branch a pull request's source ref names.
fn branch_name(r: &str) -> &str {
    r.strip_prefix("refs/heads/").unwrap_or(r)
}

/// Azure DevOps' row under `pos` (None: the cursor's, for `?`): a task's,
/// a pull request's or a branch's menu (matrix §5.10–5.12).
fn ado_row_menu(app: &mut App, pos: Option<Position>) -> Option<(String, Vec<MenuItem>)> {
    let Some(Overlay::Ado(view)) = &mut app.modals.overlay else {
        return None;
    };
    if view.threads.is_some() || view.voting.is_some() {
        return None;
    }
    if let Some(pos) = pos {
        if !view.list_area.contains(pos) {
            return None;
        }
        let row = view.scroll + usize::from(pos.y - view.list_area.y);
        if row >= view.len() {
            return None;
        }
        view.select(row);
    }
    let view = match &app.modals.overlay {
        Some(Overlay::Ado(view)) => view.clone(),
        _ => return None,
    };
    let project = view.project.clone()?;
    if let Some(task) = view.task() {
        return Some((format!("task #{}", task.id), ado_task_items(task)));
    }
    if let Some(pr) = view.pr() {
        return Some((
            format!("pull request !{}", pr.id),
            ado_pr_items(app, &project, &view, pr),
        ));
    }
    let branch = view.branch()?;
    Some((
        format!("branch {}", branch.name),
        ado_branch_items(app, &project, &view, branch),
    ))
}

/// A sprint task's menu: start a session on it (the branch, worktree and
/// agent `kete ado start-task` cuts) while it is open, the browser, its
/// id, link and title.
fn ado_task_items(task: &crate::ado::AdoTask) -> Vec<MenuItem> {
    let mut items = Vec::new();
    let finished = ["Done", "Closed", "Completed", "Removed", "Resolved"]
        .iter()
        .any(|s| task.state.eq_ignore_ascii_case(s));
    if !finished {
        items.push(MenuItem {
            key: Some("s s".into()),
            ..MenuItem::new(
                "Start session\u{2026}",
                MenuAction::OverlayKeys(vec![key('s'), key('s')]),
            )
        });
    }
    items.push(MenuItem::new(
        "Open in Azure DevOps",
        MenuAction::OpenLink(task.url.clone()),
    ));
    items.extend(copies(&[
        ("Id", format!("#{}", task.id)),
        ("Link", task.url.clone()),
        ("Title", task.title.clone()),
    ]));
    items
}

/// An Azure DevOps pull request's menu: the browser, its threads, where
/// its work is (or a worktree cut on its branch), its link, number and
/// branch, a vote on someone else's, the merge from the checkout on its
/// branch, the lists read again.
fn ado_pr_items(
    app: &App,
    project: &ProjectId,
    view: &crate::ado::AdoView,
    pr: &crate::ado::AdoPr,
) -> Vec<MenuItem> {
    let branch = branch_name(&pr.source).to_string();
    let mut items = vec![
        MenuItem::new("Open in browser", MenuAction::OpenLink(pr.url.clone())),
        verbs::overlay_key_row(
            "Review comments\u{2026}",
            KeyCode::Char('c'),
            KeyModifiers::NONE,
            false,
        ),
    ];
    let (go, on) = go_rows(app, project, &branch, false);
    items.extend(go);
    if !on {
        if let Some(b) = known_branch(view, &branch) {
            items.push(checkout_row(project, b));
        }
    }
    items.extend(copies(&[
        ("Link", pr.url.clone()),
        ("Number", format!("!{}", pr.id)),
        ("Branch name", branch.clone()),
    ]));
    if !pr.is_draft && !pr.mine {
        let votes = crate::ado::VOTES
            .iter()
            .map(|(c, _, label)| {
                let mut word = label.to_string();
                if let Some(first) = word.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                MenuItem {
                    key: Some(format!("v {c}")),
                    ..MenuItem::new(word, MenuAction::OverlayKeys(vec![key('v'), key(*c)]))
                }
            })
            .collect();
        items.push(verbs::submenu("Vote", votes));
    }
    if !pr.is_draft {
        if let Some(w) = worktree_on(app, project, &branch) {
            items.push(MenuItem::new(
                "Merge\u{2026}",
                MenuAction::MergePullRequest {
                    worktree: w.id.clone(),
                    agent: None,
                },
            ));
        }
    }
    items.push(verbs::overlay_key_row(
        "Refresh",
        KeyCode::Char('r'),
        KeyModifiers::NONE,
        false,
    ));
    items
}

/// The branch `name` as the Branches tab read it, when it has.
fn known_branch<'a>(
    view: &'a crate::ado::AdoView,
    name: &str,
) -> Option<&'a crate::ado::AdoBranch> {
    match &view.branches {
        crate::ado::Load::Ready(list) => list.iter().find(|b| b.name == name),
        _ => None,
    }
}

/// **Check out in new worktree…** for branch `b`: cut from the remote's
/// when only the remote has it, so it tracks it.
fn checkout_row(project: &ProjectId, b: &crate::ado::AdoBranch) -> MenuItem {
    MenuItem::new(
        "Check out in new worktree\u{2026}",
        MenuAction::CheckoutBranch {
            project: project.clone(),
            branch: b.name.clone(),
            remote: if b.local { None } else { b.remote.clone() },
        },
    )
}

/// A branch's menu: the worktree it is checked out in (or one cut on it),
/// the session at work there, its pull request, its name, its page.
fn ado_branch_items(
    app: &App,
    project: &ProjectId,
    view: &crate::ado::AdoView,
    b: &crate::ado::AdoBranch,
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    let (go, on) = go_rows(app, project, &b.name, false);
    if !on {
        items.push(checkout_row(project, b));
    }
    // The worktree first: it is the branch's own place.
    let (worktree, session): (Vec<MenuItem>, Vec<MenuItem>) = go
        .into_iter()
        .partition(|i| matches!(i.action, MenuAction::Jump(PaletteTarget::Worktree(_))));
    items.extend(worktree);
    items.extend(session);
    if let crate::ado::Load::Ready(prs) = &view.prs {
        if let Some(pr) = prs.iter().find(|p| branch_name(&p.source) == b.name) {
            items.push(MenuItem::new(
                format!("Open pull request !{}", pr.id),
                MenuAction::OpenLink(pr.url.clone()),
            ));
        }
    }
    items.extend(copies(&[("Name", b.name.clone())]));
    if let Some(url) = branch_page(view, &b.name) {
        items.push(MenuItem::new(
            "Open in Azure DevOps",
            MenuAction::OpenLink(url),
        ));
    }
    items
}

/// A branch's page on Azure DevOps (`…/_git/<repo>?version=GB<name>`),
/// read off a pull request of the same repo the overlay has loaded — a
/// menu never runs `git` on the loop to find the remote. None until the
/// pull requests are in.
fn branch_page(view: &crate::ado::AdoView, name: &str) -> Option<String> {
    let crate::ado::Load::Ready(prs) = &view.prs else {
        return None;
    };
    let repo = prs
        .iter()
        .find_map(|p| p.url.split_once("/pullrequest/").map(|(repo, _)| repo))?;
    Some(format!("{repo}?version=GB{name}"))
}

// ---- the pull requests' and the issues' modals ----

/// The pull requests modal's row under `pos`: the cursor there, and its
/// menu — the modal's own keys for a session on it, its diff, a comment,
/// a refresh — with where its work is and its copies.
fn pr_modal_row_menu(app: &mut App, pos: Position) -> Option<(String, Vec<MenuItem>)> {
    let pr = crate::pr_modal::select_at(app, pos)?;
    let project = match &app.modals.overlay {
        Some(Overlay::PullRequests(view)) => view.project.clone(),
        _ => return None,
    };
    let ctrl = |c: char, label: &str| {
        verbs::overlay_key_row(label, KeyCode::Char(c), KeyModifiers::CONTROL, false)
    };
    let mut items = vec![
        ctrl('o', "Open in browser"),
        verbs::submenu(
            "New session on this pull request",
            vec![
                verbs::overlay_key_row(
                    "Quick prompt\u{2026}",
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    false,
                ),
                verbs::overlay_key_row(
                    "Pick a harness\u{2026}",
                    KeyCode::Tab,
                    KeyModifiers::NONE,
                    false,
                ),
                verbs::overlay_key_row(
                    "From an agent preset\u{2026}",
                    KeyCode::BackTab,
                    KeyModifiers::SHIFT,
                    false,
                ),
            ],
        ),
    ];
    let (go, on) = go_rows(app, &project, &pr.head, false);
    items.extend(go);
    items.push(ctrl('g', "View diff"));
    items.extend(copies(&[
        ("Link", pr.url.clone()),
        ("Number", format!("#{}", pr.number)),
        ("Branch name", pr.head.clone()),
    ]));
    items.push(ctrl('c', "Comment\u{2026}"));
    if on && !pr.is_draft {
        if let Some(w) = worktree_on(app, &project, &pr.head) {
            items.push(MenuItem::new(
                "Merge\u{2026}",
                MenuAction::MergePullRequest {
                    worktree: w.id.clone(),
                    agent: None,
                },
            ));
        }
    }
    items.push(ctrl('r', "Refresh pull requests"));
    Some((format!("pull request #{}", pr.number), items))
}

/// The issues modal's row under `pos`: the browser, a session on it, its
/// link and number.
fn issue_modal_row_menu(app: &mut App, pos: Position) -> Option<(String, Vec<MenuItem>)> {
    let issue = crate::issues::select_at(app, pos)?;
    let mut items = vec![
        verbs::overlay_key_row(
            "Open in browser",
            KeyCode::Char('o'),
            KeyModifiers::CONTROL,
            false,
        ),
        verbs::submenu(
            "New session on this issue",
            vec![
                verbs::overlay_key_row(
                    "Quick prompt\u{2026}",
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    false,
                ),
                verbs::overlay_key_row(
                    "From an agent preset\u{2026}",
                    KeyCode::BackTab,
                    KeyModifiers::SHIFT,
                    false,
                ),
            ],
        ),
    ];
    items.extend(copies(&[
        ("Link", issue.url.clone()),
        ("Number", format!("#{}", issue.number)),
    ]));
    let ctrl = |c: char, label: &str| {
        verbs::overlay_key_row(label, KeyCode::Char(c), KeyModifiers::CONTROL, false)
    };
    items.push(ctrl('c', "Comment\u{2026}"));
    items.push(ctrl('e', "Edit issue\u{2026}"));
    items.push(ctrl('r', "Refresh issues"));
    Some((format!("issue #{}", issue.number), items))
}

// ---- an overlay's own keys, after its row's menu in `?` ----

/// The keys an overlay answers that are no row's: `?` lists them after
/// the row's menu. (label, key, modifiers, destructive)
type KeyRow = (&'static str, KeyCode, KeyModifiers, bool);

const DIFF_KEYS: &[KeyRow] = &[
    (
        "Compare with the base or HEAD",
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
        false,
    ),
    (
        "Next change",
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
        false,
    ),
    (
        "Previous change",
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
        false,
    ),
    (
        "Accept the change (stage it)",
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
        false,
    ),
    (
        "Reject the change\u{2026}",
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
        true,
    ),
    (
        "Tree or flat list",
        KeyCode::Char('t'),
        KeyModifiers::CONTROL,
        false,
    ),
    (
        "Next layout",
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
        false,
    ),
    (
        "Full context or folded",
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
        false,
    ),
    ("Close the diff", KeyCode::Esc, KeyModifiers::NONE, false),
];

const ACCOUNT_KEYS: &[KeyRow] = &[
    (
        "Add an account\u{2026}",
        KeyCode::Char('a'),
        KeyModifiers::NONE,
        false,
    ),
    ("Spread mode", KeyCode::Char('s'), KeyModifiers::NONE, false),
    (
        "Spread threshold",
        KeyCode::Char('t'),
        KeyModifiers::NONE,
        false,
    ),
    ("Close", KeyCode::Esc, KeyModifiers::NONE, false),
];

/// The accounts overlay's Wake view, on its picked account: the keys
/// `crate::wake_tab` answers, which are not the list's (lane/wake).
const WAKE_KEYS: &[KeyRow] = &[
    ("On or off", KeyCode::Char(' '), KeyModifiers::NONE, false),
    ("Earlier", KeyCode::Left, KeyModifiers::NONE, false),
    ("Later", KeyCode::Right, KeyModifiers::NONE, false),
    (
        "Suggested hour",
        KeyCode::Char('s'),
        KeyModifiers::NONE,
        false,
    ),
    ("Wake up now", KeyCode::Char('w'), KeyModifiers::NONE, false),
    ("Accounts", KeyCode::Tab, KeyModifiers::NONE, false),
];

/// The Wake view's own key after its row's, for `?`.
const CLOSE_KEY: KeyRow = ("Close", KeyCode::Esc, KeyModifiers::NONE, false);

const ADO_KEYS: &[KeyRow] = &[
    ("Tasks", KeyCode::Char('1'), KeyModifiers::NONE, false),
    (
        "Pull requests",
        KeyCode::Char('2'),
        KeyModifiers::NONE,
        false,
    ),
    ("Branches", KeyCode::Char('3'), KeyModifiers::NONE, false),
    ("Read again", KeyCode::Char('r'), KeyModifiers::NONE, false),
    ("Close", KeyCode::Esc, KeyModifiers::NONE, false),
];

fn keys_rows(table: &[KeyRow]) -> Vec<MenuItem> {
    table
        .iter()
        .map(|(label, code, mods, destructive)| {
            verbs::overlay_key_row(label, *code, *mods, *destructive)
        })
        .collect()
}
