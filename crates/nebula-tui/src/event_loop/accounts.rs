//! The ACCOUNTS OVERLAY's keys, and the usage the daemon pushes
//! (`ServerEvent::AccountUsage` / `SessionContexts` / `SessionStops`).
//! State and rows live in `crate::accounts_overlay`.

use super::*;
use crate::accounts_overlay::{
    build_rows, AccountForm, AccountsHit, AccountsView, Field, FormKind, Mode, Outcome, Tab,
};
use crate::wake_tab::Saving;

/// The accounts to show: the pushed list, or the kept files when the
/// daemon is too old to push one.
fn readings(app: &mut App) -> Vec<nebula_core::usage::AccountUsage> {
    if !app.launcher.usage_pushed {
        app.launcher.account_usage = crate::accounts_overlay::kept_readings();
    }
    app.launcher.account_usage.clone()
}

/// The rows, and every setting the overlay mirrors, read again: the
/// profiles, the spread, each project's default accounts.
fn rows(app: &mut App) -> Vec<crate::accounts_overlay::AccountRow> {
    let cfg = crate::config::Config::load();
    crate::quick_prompt::mirror_accounts(app, &cfg);
    let usage = readings(app);
    build_rows(
        &cfg.profiles,
        &usage,
        &app.tree.agents,
        crate::accounts_overlay::login_lookup,
    )
}

/// Fill the view's mirrors from the app and the config.
fn mirror(app: &App, view: &mut AccountsView) {
    let cfg = crate::config::Config::load();
    view.harnesses = cfg
        .harness_registry()
        .iter()
        .map(|h| h.id.clone())
        .collect();
    view.spread = cfg.account_spread.clone();
    view.threshold = cfg.account_spread_threshold.clone();
    view.projects = app
        .tree
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.repo_path.clone()))
        .collect();
    view.project_accounts = app.launcher.project_accounts.clone();
    view.sessions_follow = app
        .orch
        .daemon_protocol
        .is_some_and(|v| v >= nebula_core::RENAME_PROFILE_PROTOCOL);
}

/// `⇧U`: open the overlay.
pub(crate) fn open(app: &mut App) {
    let rows = rows(app);
    let (config, local) = crate::config::Config::layer_paths();
    let mut view = AccountsView::new(rows, config, local);
    mirror(app, &mut view);
    app.modals.overlay = Some(Overlay::Accounts(view));
    app.chrome.dirty = true;
}

/// **Account details…**: the overlay with its cursor on account `key`.
pub(crate) fn open_on(app: &mut App, key: &str) {
    open(app);
    if let Some(Overlay::Accounts(view)) = &mut app.modals.overlay {
        if let Some(i) = view.rows.iter().position(|r| r.key == key) {
            view.cursor = i;
        }
    }
}

/// Rebuild the open overlay's rows (a push, an edit), keeping the cursor.
pub(crate) fn refresh(app: &mut App) {
    if !matches!(app.modals.overlay, Some(Overlay::Accounts(_))) {
        return;
    }
    let rows = rows(app);
    let Some(Overlay::Accounts(mut view)) = app.modals.overlay.take() else {
        return;
    };
    view.set_rows(rows);
    mirror(app, &mut view);
    app.modals.overlay = Some(Overlay::Accounts(view));
}

/// Turn the overlay to `tab`. The Wake view reads its status again each
/// time it comes up, with the cursor on the account the Accounts view had
/// picked; going back, the Accounts cursor follows the Wake one.
fn show_tab(app: &mut App, tab: Tab) {
    let jobs = app.jobs.view_jobs.clone();
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return;
    };
    view.tab = tab;
    match tab {
        Tab::Wake => {
            view.wake.focus = view.selected().map(|r| r.key.clone());
            view.wake.select_focus();
            view.wake.loading = true;
            if let Some(jobs) = &jobs {
                crate::wake_tab::spawn_status(jobs, &view.wake.inbox);
            }
        }
        Tab::Accounts => {
            let at = view
                .wake
                .selected_key()
                .and_then(|key| view.rows.iter().position(|r| r.key == key));
            if let Some(at) = at {
                view.cursor = at;
            }
        }
    }
    app.chrome.dirty = true;
}

/// Open the overlay straight onto its Wake view: what the registry's
/// wake-up schedule verb does.
pub(crate) fn open_wake_tab(app: &mut App) {
    if !matches!(app.modals.overlay, Some(Overlay::Accounts(_))) {
        open(app);
    }
    if let Some(Overlay::Accounts(view)) = &mut app.modals.overlay {
        view.mode = Mode::List;
    }
    show_tab(app, Tab::Wake);
}

/// **Wake-up schedule…** on an account's menu: the Wake view, its cursor
/// on that account (once the status read lists it).
pub(crate) fn open_wake_tab_on(app: &mut App, key: &str) {
    if !matches!(app.modals.overlay, Some(Overlay::Accounts(_))) {
        open(app);
    }
    if let Some(Overlay::Accounts(view)) = &mut app.modals.overlay {
        view.mode = Mode::List;
        if let Some(i) = view.rows.iter().position(|r| r.key == key) {
            view.cursor = i;
        }
    }
    show_tab(app, Tab::Wake);
}

/// **Wake up now** on an account's menu: that account picked on the view
/// the overlay shows (opened if it is not up), then woken as `w` wakes it.
/// Never another account: one the view can't pick yet says why instead.
pub(crate) fn wake_account_key(app: &mut App, key: &str) {
    if !matches!(app.modals.overlay, Some(Overlay::Accounts(_))) {
        open(app);
    }
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return;
    };
    view.mode = Mode::List;
    let picked = match view.tab {
        Tab::Accounts => match view.rows.iter().position(|r| r.key == key) {
            Some(i) => {
                view.cursor = i;
                true
            }
            None => false,
        },
        Tab::Wake => {
            view.wake.focus = Some(key.to_string());
            view.wake.select_focus();
            view.wake.selected_key().as_deref() == Some(key)
        }
    };
    if picked {
        wake_selected(app);
    } else {
        app.chrome.flash = Some(format!("no account {key} to wake yet"));
        app.chrome.dirty = true;
    }
}

/// The registry's `wake_account` verb: Wake now on the account the open
/// overlay has picked, else the overlay opened on its Wake view to pick one.
pub(crate) fn wake_account_verb(app: &mut App) {
    let picked = matches!(
        &app.modals.overlay,
        Some(Overlay::Accounts(v)) if v.mode == Mode::List && v.has_account()
    );
    if picked {
        wake_selected(app);
    } else {
        open_wake_tab(app);
    }
}

/// `w`: wake the picked account now, on a thread (`kete wake <account>
/// --force` can take two minutes). The footer says so at once, then how it
/// went; a second press while it runs waits.
fn wake_selected(app: &mut App) {
    let jobs = app.jobs.view_jobs.clone();
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return;
    };
    let account = match view.wake_target() {
        Ok(account) => account,
        Err(why) => {
            app.chrome.flash = Some(why);
            return;
        }
    };
    if !view.wake.inbox.begin_wake(&account) {
        app.chrome.flash = Some(format!("already waking {account}\u{2026}"));
        return;
    }
    app.chrome.flash = Some(format!("waking {account}\u{2026}"));
    match jobs {
        Some(jobs) => crate::wake_tab::spawn_wake(&jobs, &view.wake.inbox, account),
        // No loop to answer (the unit tests): nothing is started.
        None => view.wake.inbox.end_wake(&account),
    }
}

/// An edit on the Wake view was queued: start the thread that waits out
/// the debounce and saves, when none is waiting. With no loop to run it
/// (the unit tests) nothing is started, and nothing is ever saved.
fn start_saving(app: &mut App, saving: Saving) {
    if saving != Saving::Start {
        return;
    }
    let Some(jobs) = app.jobs.view_jobs.clone() else {
        return;
    };
    if let Some(Overlay::Accounts(view)) = &app.modals.overlay {
        crate::wake_tab::spawn_flush(&jobs, &view.wake.inbox);
    }
}

/// What a key did: a message for the footer, an account to sign in.
type Done = Option<Result<Outcome, String>>;

/// Why a rename waits for the daemon: only it knows every session that
/// names the account, and one left on the old name could not resume.
const RENAME_OFFLINE: &str =
    "not connected to the kete daemon: a rename checks the sessions on the account first";

/// The list's keys: j/k move, Space/e switch a profile on or off, d/*
/// make it the default, y keeps it for your own sessions (or gives it
/// back), a add, Enter edit, c duplicate, r rename, x/Delete
/// remove, l sign in, p use for a repo, m move the grid's session onto it,
/// s/t the spread and its threshold, w wakes the account now, Tab turns to
/// the Wake view, Esc (or q) closes.
fn list_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) -> Done {
    let agents = app.tree.agents.clone();
    let connected = !matches!(app.chrome.conn, crate::app::ConnState::Disconnected);
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return None;
    };
    let said = |r: Result<String, String>| {
        Some(r.map(|m| Outcome {
            message: m,
            ..Outcome::default()
        }))
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.modals.overlay = None;
            None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            view.step(1);
            None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            view.step(-1);
            None
        }
        KeyCode::Char(' ') | KeyCode::Char('e') => said(view.toggle_enabled()),
        KeyCode::Char('d') | KeyCode::Char('*') => said(view.set_default()),
        KeyCode::Char('y') => said(view.toggle_reserved()),
        KeyCode::Char('a') => {
            view.open_add();
            None
        }
        KeyCode::Enter => view.open_edit().err().map(Err),
        KeyCode::Char('c') => view.open_duplicate().err().map(Err),
        KeyCode::Char('r') if !connected => Some(Err(RENAME_OFFLINE.into())),
        KeyCode::Char('r') => view.open_rename(&agents).err().map(Err),
        KeyCode::Char('x') | KeyCode::Delete => view.open_remove(&agents).err().map(Err),
        KeyCode::Char('p') => view.open_use_for().err().map(Err),
        KeyCode::Char('s') => Some(view.cycle_spread()),
        KeyCode::Char('t') => Some(view.cycle_threshold()),
        KeyCode::Char('l') => {
            let key = view.selected().map(|r| r.key.clone());
            if let Some(key) = key {
                sign_in(app, &key, out);
            }
            None
        }
        KeyCode::Char('m') => {
            move_session_here(app);
            None
        }
        KeyCode::Char('w') => {
            wake_selected(app);
            None
        }
        KeyCode::Tab | KeyCode::BackTab => {
            show_tab(app, Tab::Wake);
            None
        }
        _ => None,
    }
}

/// The Wake view's keys: j/k (or ↑/↓) walk the accounts, Space or Enter
/// switch the account's wake-up on or off, ←/→ move its hour a quarter
/// hour (⇧ an hour), s snaps it to the suggested hour, w wakes the account
/// now, Tab turns back to the accounts, Esc (or q) closes. An edit shows
/// at once and is saved on a thread (`crate::wake_tab`).
fn wake_key(app: &mut App, key: KeyEvent) -> Done {
    let shifted = key.modifiers.contains(KeyModifiers::SHIFT);
    let steps = |delta: i32| if shifted { delta * 4 } else { delta };
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return None;
    };
    let saving = match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.modals.overlay = None;
            return None;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            show_tab(app, Tab::Accounts);
            return None;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            view.wake.step(1);
            return None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            view.wake.step(-1);
            return None;
        }
        KeyCode::Left => view.wake.nudge(steps(-1)),
        KeyCode::Right => view.wake.nudge(steps(1)),
        KeyCode::Char(' ') | KeyCode::Enter => view.wake.toggle(),
        KeyCode::Char('s') => match view.wake.snap() {
            Ok(saving) => saving,
            Err(why) => return Some(Err(why)),
        },
        KeyCode::Char('w') => {
            wake_selected(app);
            return None;
        }
        _ => return None,
    };
    start_saving(app, saving);
    None
}

/// A form's keys: Tab / ↓ / ↑ walk the fields, ←/→ (or Space) step the
/// harness and flip a box, typing fills a text field, Delete clears an
/// edit's variables, Enter saves, Esc goes back to the list.
fn form_key(app: &mut App, key: KeyEvent) -> Done {
    let agents = app.tree.agents.clone();
    let connected = !matches!(app.chrome.conn, crate::app::ConnState::Disconnected);
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return None;
    };
    let taken: Vec<String> = view
        .rows
        .iter()
        .filter_map(|r| r.profile.as_ref().map(|p| p.name.trim().to_string()))
        .collect();
    let harnesses = view.harnesses.clone();
    if key.code == KeyCode::Enter {
        let renaming =
            matches!(&view.mode, Mode::Form(f) if matches!(f.kind, FormKind::Rename { .. }));
        if renaming && !connected {
            return Some(Err(RENAME_OFFLINE.into()));
        }
        return Some(view.submit_form(&agents));
    }
    let Mode::Form(form) = &mut view.mode else {
        return None;
    };
    let n = form.fields().len();
    let field = form.current();
    match key.code {
        KeyCode::Esc => view.mode = Mode::List,
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % n,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + n - 1) % n,
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if field == Field::Harness => {
            if !harnesses.is_empty() {
                let at = harnesses
                    .iter()
                    .position(|h| *h == form.harness)
                    .unwrap_or(0);
                let next = if key.code == KeyCode::Left {
                    (at + harnesses.len() - 1) % harnesses.len()
                } else {
                    (at + 1) % harnesses.len()
                };
                form.harness = harnesses[next].clone();
                if form.kind == FormKind::Add && !form.touched {
                    form.name = crate::accounts_overlay::suggest_name(&form.harness, &taken);
                }
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if field == Field::Adopt => {
            form.adopt = !form.adopt;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if field == Field::Default => {
            form.make_default = !form.make_default;
        }
        KeyCode::Delete if form.can_clear_env() => {
            form.clear_env = true;
            form.env.clear();
            form.field = form
                .fields()
                .iter()
                .position(|f| *f == Field::Env)
                .unwrap_or(form.field);
        }
        KeyCode::Backspace if AccountForm::is_text(field) => {
            let text = if field == Field::Name {
                &mut form.name
            } else {
                &mut form.env
            };
            text.pop();
            form.touched = true;
        }
        KeyCode::Char(c)
            if AccountForm::is_text(field)
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if field == Field::Name {
                form.name.push(c);
            } else {
                form.env.push(c);
            }
            form.touched = true;
        }
        _ => {}
    }
    if let Mode::Form(form) = &mut view.mode {
        form.error = None;
    }
    None
}

/// The remove's keys: y / Enter remove (the home stays), p removes and
/// deletes the home when there is one, n / Esc keep.
fn remove_key(app: &mut App, key: KeyEvent) -> Done {
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return None;
    };
    let has_home = matches!(&view.mode, Mode::Remove { home: Some(_), .. });
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => Some(view.confirm_remove(false)),
        KeyCode::Char('p') if has_home => Some(view.confirm_remove(true)),
        KeyCode::Char('n') | KeyCode::Esc => {
            view.mode = Mode::List;
            None
        }
        _ => None,
    }
}

/// "Use for repo"'s keys: j/k walk the repos, Enter sets or clears, Esc
/// goes back.
fn use_for_key(app: &mut App, key: KeyEvent) -> Done {
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return None;
    };
    let n = view.projects.len();
    let Mode::UseFor { cursor } = &mut view.mode else {
        return None;
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => view.mode = Mode::List,
        KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(n.saturating_sub(1)),
        KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
        KeyCode::Enter | KeyCode::Char(' ') => return Some(view.toggle_use_for()),
        _ => {}
    }
    None
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) {
    let Some(Overlay::Accounts(view)) = &app.modals.overlay else {
        return;
    };
    let done = match view.mode {
        Mode::List if view.tab == Tab::Wake => wake_key(app, key),
        Mode::List => list_key(app, key, out),
        Mode::Form(_) => form_key(app, key),
        Mode::Remove { .. } => remove_key(app, key),
        Mode::UseFor { .. } => use_for_key(app, key),
    };
    match done {
        Some(Ok(outcome)) => {
            app.chrome.flash = Some(outcome.message);
            refresh(app);
            if let Some(account) = outcome.sign_in {
                sign_in(app, &account, out);
            }
            // The renamed profile's sessions follow it, in the daemon.
            if let Some((old, new)) = outcome.rename_sessions {
                send(app, out, |req_id| ClientRequest::RenameProfileSessions {
                    req_id,
                    old,
                    new,
                });
            }
        }
        Some(Err(message)) => app.chrome.flash = Some(message),
        None => {}
    }
    app.chrome.dirty = true;
}

/// A click: a button is its key, pressed; a row puts the cursor there; a
/// form's field takes the form's cursor; a repo in "use for" is picked
/// (a second click sets it). The wheel walks the rows.
pub(crate) fn handle_mouse(app: &mut App, mouse: MouseEvent, out: &mut Vec<ClientRequest>) -> bool {
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return false;
    };
    app.chrome.dirty = true;
    let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::ScrollDown if view.mode == Mode::List => view.step(1),
        MouseEventKind::ScrollUp if view.mode == Mode::List => view.step(-1),
        MouseEventKind::Down(MouseButton::Left) => {
            let Some(hit) = view
                .hits
                .iter()
                .find(|(r, _)| r.contains(pos))
                .map(|(_, h)| *h)
            else {
                return true;
            };
            match hit {
                AccountsHit::Key(code) => {
                    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE), out);
                }
                AccountsHit::Row(i) => view.cursor = i.min(view.rows.len().saturating_sub(1)),
                AccountsHit::Field(i) => {
                    if let Mode::Form(form) = &mut view.mode {
                        form.field = i.min(form.fields().len().saturating_sub(1));
                    }
                }
                AccountsHit::Project(i) => {
                    if let Mode::UseFor { cursor } = &mut view.mode {
                        if *cursor == i {
                            handle_key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), out);
                        } else {
                            *cursor = i;
                        }
                    }
                }
                AccountsHit::Tab(tab) => show_tab(app, tab),
                AccountsHit::WakeRow(i) => view.wake.select(i),
                AccountsHit::WakeSwitch(i) => {
                    view.wake.select(i);
                    handle_key(
                        app,
                        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
                        out,
                    );
                }
                AccountsHit::WakeAnchor(i, direction) => {
                    view.wake.select(i);
                    let code = if direction < 0 {
                        KeyCode::Left
                    } else {
                        KeyCode::Right
                    };
                    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE), out);
                }
                AccountsHit::Spread(i) => {
                    let word = nebula_core::accounts::SPREADS[i.min(2)];
                    let done = view.set_spread(word);
                    match done {
                        Ok(o) => {
                            app.chrome.flash = Some(o.message);
                            refresh(app);
                        }
                        Err(e) => app.chrome.flash = Some(e),
                    }
                }
            }
        }
        _ => {}
    }
    true
}

/// The checkout a login terminal opens in: the selected one, else the
/// first project's root.
fn login_worktree(app: &App) -> Option<WorktreeId> {
    app.selected_worktree()
        .map(|w| w.id.clone())
        .filter(|id| !app.is_placeholder_worktree(id))
        .or_else(|| {
            app.tree
                .projects
                .first()
                .and_then(|p| app.root_worktree(&p.id))
        })
}

/// Sign the account `key` in: its login command (looked up here, never
/// typed by anyone) run in a new terminal session, which the pane shows.
/// With no checkout to open one in, the footer says the command instead.
pub(crate) fn sign_in(app: &mut App, key: &str, out: &mut Vec<ClientRequest>) {
    let row = match &app.modals.overlay {
        Some(Overlay::Accounts(view)) => view.rows.iter().find(|r| r.key == key).cloned(),
        _ => None,
    };
    let Some(row) = row else {
        app.chrome.flash = Some(format!("no account `{key}`"));
        return;
    };
    if let Some(problem) = &row.problem {
        app.chrome.flash = Some(format!("{}: {problem}", row.label()));
        return;
    }
    let Some(line) = nebula_core::accounts::login_command(row.profile.as_ref(), &row.harness)
    else {
        app.chrome.flash = Some(format!(
            "kete knows no sign-in for {}: sign in from a session under it",
            row.harness
        ));
        return;
    };
    let Some(worktree) = login_worktree(app) else {
        app.chrome.flash = Some(format!("add a repo first, or run it yourself: {line}"));
        return;
    };
    app.modals.overlay = None;
    app.chrome.flash = Some(format!(
        "signing {} in: finish in the terminal, then ⇧U shows it signed in",
        row.label()
    ));
    send_with(app, out, PendingIntent::SignIn { line }, |req_id| {
        ClientRequest::CreateTerminal {
            req_id,
            worktree,
            name: Some(format!("sign in {}", row.key.trim_start_matches('@'))),
        }
    });
}

/// `m`: the session under the grid's cursor moved onto the selected
/// account, through MOVE TO ACCOUNT's confirm.
fn move_session_here(app: &mut App) {
    let Some(Overlay::Accounts(view)) = &app.modals.overlay else {
        return;
    };
    let Some(row) = view.selected().cloned() else {
        return;
    };
    let session = match app.selected_session_row() {
        Some(crate::app::SessionRow::Agent(a)) => a,
        _ => {
            app.chrome.flash = Some("no session under the cursor to move".into());
            return;
        }
    };
    let harness = nebula_core::accounts::session_harness(&session);
    if harness != row.harness {
        app.chrome.flash = Some(format!(
            "{} is a {} account: {} runs {harness}",
            row.label(),
            row.harness,
            session.name
        ));
        return;
    }
    if nebula_core::accounts::session_account(&session) == row.key {
        app.chrome.flash = Some(format!("{} already runs on {}", session.name, row.label()));
        return;
    }
    let profile = row.profile.as_ref().map(|p| p.name.trim().to_string());
    crate::account_switch::confirm(app, session.id.clone(), profile);
}

/// A 55+ daemon's account list: replaces what was known.
pub(crate) fn on_account_usage(app: &mut App, accounts: Vec<nebula_core::usage::AccountUsage>) {
    app.launcher.account_usage = accounts;
    app.launcher.usage_pushed = true;
    refresh(app);
    app.chrome.dirty = true;
}

/// Session contexts: folded in by agent.
pub(crate) fn on_session_contexts(
    app: &mut App,
    contexts: Vec<nebula_core::usage::SessionContext>,
) {
    for c in contexts {
        app.launcher.session_contexts.insert(c.agent, c.context);
    }
    app.chrome.dirty = true;
}

/// Session stop notes: folded in by agent, a `None` note taking that
/// session's away.
pub(crate) fn on_session_stops(app: &mut App, stops: Vec<nebula_core::stop::SessionStop>) {
    for s in stops {
        match s.note {
            Some(note) => {
                app.launcher.session_stops.insert(s.agent, note);
            }
            None => {
                app.launcher.session_stops.remove(&s.agent);
            }
        }
    }
    app.chrome.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::super::tests::{buffer_text, press, seed_tree, with_config_json};
    use super::*;
    use nebula_core::usage::{
        AccountUsage, ContextUsage, SessionContext, UsageSource, UsageWindow, WindowKind,
    };
    use ratatui::backend::TestBackend;

    fn own_login(percent: f32) -> AccountUsage {
        own_login_read(percent, nebula_core::clock::now_secs() as i64)
    }

    fn own_login_read(percent: f32, read_at: i64) -> AccountUsage {
        AccountUsage {
            account: "@claude".into(),
            harness: "claude".into(),
            windows: vec![
                UsageWindow {
                    kind: WindowKind::Session,
                    label: None,
                    percent,
                    resets_at: None,
                    read_at,
                    estimated: false,
                },
                UsageWindow {
                    kind: WindowKind::Weekly,
                    label: None,
                    percent: 7.0,
                    resets_at: None,
                    read_at,
                    estimated: false,
                },
            ],
            source: UsageSource::Statusline,
            at: 0,
            plan: None,
        }
    }

    /// The daemon's pushes land: the list replaces, contexts fold by id,
    /// and an open overlay follows the push.
    #[test]
    fn pushed_usage_and_contexts_fold_into_the_app_and_the_overlay() {
        with_config_json("{}", || {
            let mut app = App::new();
            let mut out = Vec::new();
            press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT, &mut out);
            assert!(matches!(app.modals.overlay, Some(Overlay::Accounts(_))));
            handle_server_event(
                &mut app,
                ServerEvent::AccountUsage {
                    accounts: vec![own_login(21.0)],
                },
                &mut out,
            );
            assert!(app.launcher.usage_pushed);
            let Some(Overlay::Accounts(view)) = &app.modals.overlay else {
                panic!("closed");
            };
            assert_eq!(view.rows.len(), 1, "the own login shows with no profiles");
            assert_eq!(view.rows[0].key, "@claude");
            let ctx = |p: f32| ContextUsage {
                percent: Some(p),
                tokens: None,
                window: None,
                compactions: 0,
                last_compact_at: None,
                source: UsageSource::Statusline,
                at: 0,
            };
            for p in [10.0, 30.0] {
                handle_server_event(
                    &mut app,
                    ServerEvent::SessionContexts {
                        contexts: vec![SessionContext {
                            agent: AgentId("a1".into()),
                            context: ctx(p),
                        }],
                    },
                    &mut out,
                );
            }
            assert_eq!(app.launcher.session_contexts.len(), 1);
            assert_eq!(
                app.launcher.session_contexts[&AgentId("a1".into())].percent,
                Some(30.0)
            );
            assert!(out.is_empty(), "nothing goes to the daemon");
        });
    }

    /// The daemon's stop notes land by agent, a later note replaces and a
    /// `None` one takes the session's away; a Snapshot clears them (the
    /// daemon sends the list again after it) and so does the session's
    /// removal.
    #[test]
    fn pushed_stop_notes_fold_in_and_clear_with_a_none_a_snapshot_or_the_session() {
        use nebula_core::stop::{SessionStop, StopNote, AUTHENTICATION_FAILED, RATE_LIMIT};
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            let a1 = AgentId("a1".into());
            let note = |reason: &str| StopNote {
                reason: reason.into(),
                waiting: reason == RATE_LIMIT,
                resumes: (reason == RATE_LIMIT).then(|| "3:45pm".into()),
                resets_at: None,
                detail: None,
                at: 0,
            };
            let push = |note: Option<StopNote>| ServerEvent::SessionStops {
                stops: vec![SessionStop {
                    agent: a1.clone(),
                    note,
                }],
            };

            handle_server_event(&mut app, push(Some(note(RATE_LIMIT))), &mut out);
            assert_eq!(app.launcher.session_stops.get(&a1), Some(&note(RATE_LIMIT)));
            handle_server_event(&mut app, push(Some(note(AUTHENTICATION_FAILED))), &mut out);
            assert_eq!(app.launcher.session_stops.len(), 1);
            assert_eq!(
                app.launcher.session_stops.get(&a1),
                Some(&note(AUTHENTICATION_FAILED)),
                "the later note replaces"
            );
            handle_server_event(&mut app, push(None), &mut out);
            assert!(app.launcher.session_stops.is_empty(), "None takes it away");

            // A resync Snapshot: the notes it left are stale until the
            // daemon's list after it says otherwise.
            handle_server_event(&mut app, push(Some(note(RATE_LIMIT))), &mut out);
            app.chrome.snapshot_loaded = true;
            let tree = app.tree.clone();
            handle_server_event(
                &mut app,
                ServerEvent::Snapshot {
                    projects: tree.projects,
                    worktrees: tree.worktrees,
                    agents: tree.agents,
                    terminals: tree.terminals,
                    links: tree.links,
                    pr_seen: Vec::new(),
                    ui_state: None,
                    subagents: Vec::new(),
                },
                &mut out,
            );
            assert!(app.launcher.session_stops.is_empty());

            // The session removed takes its note with it.
            handle_server_event(&mut app, push(Some(note(RATE_LIMIT))), &mut out);
            assert_eq!(app.launcher.session_stops.len(), 1);
            handle_server_event(
                &mut app,
                ServerEvent::EntityRemoved {
                    id: EntityId::Agent(a1.clone()),
                },
                &mut out,
            );
            assert!(app.launcher.session_stops.is_empty());
        });
    }

    /// The overlay draws the own-login account with its two bars, and the
    /// footer names its keys; Esc closes.
    #[test]
    fn the_overlay_draws_bars_and_closes_on_esc() {
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT, &mut out);
            on_account_usage(&mut app, vec![own_login(83.0)]);
            let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 30)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains("Accounts"), "{text}");
            assert!(text.contains("claude (own login)"), "{text}");
            assert!(text.contains(" 83%"), "{text}");
            assert!(text.contains("\u{25b0}"), "{text}");
            assert!(text.contains("Space: on/off"), "{text}");
            press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE, &mut out);
            assert!(
                app.chrome
                    .flash
                    .as_deref()
                    .unwrap_or("")
                    .contains("own login"),
                "an own login cannot be switched off: {:?}",
                app.chrome.flash
            );
            press(&mut app, KeyCode::Esc, KeyModifiers::NONE, &mut out);
            assert!(app.modals.overlay.is_none());
        });
    }

    /// A session reading four hours old is drawn dim with its age inline;
    /// one past its window reads "—".
    #[test]
    fn a_stale_reading_shows_its_age_and_an_aged_out_one_reads_unknown() {
        with_config_json("{}", || {
            let now = nebula_core::clock::now_secs() as i64;
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT, &mut out);
            on_account_usage(&mut app, vec![own_login_read(83.0, now - 4 * 3600)]);
            let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 30)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains(" 83%"), "{text}");
            assert!(text.contains("read 4h ago"), "{text}");
            on_account_usage(&mut app, vec![own_login_read(83.0, now - 6 * 3600)]);
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let text = buffer_text(&terminal);
            assert!(!text.contains(" 83%"), "aged out: {text}");
            assert!(text.contains("\u{2014}"), "{text}");
        });
    }
    /// A terminal shorter than the overlay's minimum still draws: the
    /// height clamp never has its floor above its ceiling.
    #[test]
    fn the_overlay_draws_in_a_tiny_terminal() {
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT, &mut out);
            on_account_usage(&mut app, vec![own_login(40.0)]);
            for h in [1u16, 2, 5, 9, 10] {
                let mut terminal = ratatui::Terminal::new(TestBackend::new(60, h)).unwrap();
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            }
        });
    }

    /// The settings the launch-order tests stand on: `main` the harness
    /// default, `proj` the seeded project's default account, `spare` a third.
    fn launch_json(spread: &str, proj_disabled: bool) -> String {
        format!(
            r#"{{"account_spread":"{spread}","profiles":[{{"name":"main","harness":"claude","default":true}},{{"name":"spare","harness":"claude"}},{{"name":"proj","harness":"claude","disabled":{proj_disabled}}}],"projects":{{"/tmp/demo":{{"default_profiles":{{"claude":"proj"}}}}}}}}"#
        )
    }

    fn box_launch(app: &App) -> crate::quick_prompt::QuickLaunch {
        match &app.modals.overlay {
            Some(Overlay::Prompt(p)) => match &p.kind {
                PromptKind::QuickPrompt(launch) => launch.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    /// The box and the `n` picker preselect what the daemon would pick for
    /// a launch naming none: the project's default account ahead of the
    /// harness default (named under `manual`, "auto" under a spread), and
    /// the harness default again once the project's is switched off.
    #[test]
    fn a_launch_preselects_the_project_default_account() {
        let worktree = || crate::quick_prompt::QuickTarget::Worktree(WorktreeId("w1".into()));
        with_config_json(&launch_json("manual", false), || {
            let mut app = App::new();
            seed_tree(&mut app);
            crate::quick_prompt::open_for(&mut app, worktree());
            let launch = box_launch(&app);
            assert_eq!(launch.profile.as_deref(), Some("proj"));
            let label = crate::quick_prompt::profile_choice_label(
                launch.profile.as_deref(),
                &app.launcher.profiles,
                "claude",
                app.launcher.spread,
                &[],
                crate::quick_prompt::project_default(&app, &launch.target, "claude").as_deref(),
            );
            assert_eq!(label, "proj (project default)");
            assert_eq!(
                launch.profile_choices(&app.launcher.profiles, app.launcher.spread, Some("proj")),
                vec![
                    Some("proj".to_string()),
                    Some("main".to_string()),
                    Some("spare".to_string())
                ],
                "the project's leads; no \"none\" while a default stands"
            );
            // `n`: the Claude rows lead with the project's account.
            app.modals.overlay = None;
            super::super::tests::open_picker(&mut app);
            let Some(Overlay::Menu(menu)) = &app.modals.overlay else {
                panic!("{:?}", app.modals.overlay);
            };
            let first = menu
                .items
                .iter()
                .find_map(|i| match &i.action {
                    MenuAction::NewAgentOfKind {
                        kind: nebula_core::AgentKind::Claude,
                        custom: None,
                        profile,
                        ..
                    } => Some((i.label.clone(), profile.clone())),
                    _ => None,
                })
                .unwrap();
            assert_eq!(first.1.as_deref(), Some("proj"), "{first:?}");
            assert!(first.0.contains("proj (project default)"), "{first:?}");
        });
        with_config_json(&launch_json("least_used", false), || {
            let mut app = App::new();
            seed_tree(&mut app);
            crate::quick_prompt::open_for(&mut app, worktree());
            let launch = box_launch(&app);
            assert_eq!(launch.profile, None, "auto: the daemon picks");
            let label = crate::quick_prompt::profile_choice_label(
                None,
                &app.launcher.profiles,
                "claude",
                app.launcher.spread,
                &[],
                Some("proj"),
            );
            assert_eq!(label, "auto → proj (project default)");
        });
        with_config_json(&launch_json("manual", true), || {
            let mut app = App::new();
            seed_tree(&mut app);
            crate::quick_prompt::open_for(&mut app, worktree());
            assert_eq!(
                box_launch(&app).profile.as_deref(),
                Some("main"),
                "a switched-off project default falls through to the harness default"
            );
        });
    }

    /// The overlay edits the layers the test override (or a real override)
    /// routes to, never the data dir's config.json behind its back.
    #[test]
    fn the_overlay_edits_the_routed_config_layers() {
        with_config_json("{}", || {
            let mut app = App::new();
            let mut out = Vec::new();
            press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT, &mut out);
            let Some(Overlay::Accounts(view)) = &app.modals.overlay else {
                panic!("closed");
            };
            let (config, local) = crate::config::Config::layer_paths();
            assert_eq!((&view.config, &view.local), (&config, &local));
            assert_ne!(view.config, nebula_core::paths::config_path());
        });
    }

    // ---- the overlay's verbs ----

    fn view(app: &App) -> &AccountsView {
        match &app.modals.overlay {
            Some(Overlay::Accounts(v)) => v,
            other => panic!("the accounts overlay is not up: {other:?}"),
        }
    }

    fn saved() -> serde_json::Value {
        let (config, _) = crate::config::Config::layer_paths();
        serde_json::from_str(&std::fs::read_to_string(config).unwrap()).unwrap()
    }

    fn key(app: &mut App, code: KeyCode, out: &mut Vec<ClientRequest>) {
        press(app, code, KeyModifiers::NONE, out);
    }

    fn typed(app: &mut App, text: &str, out: &mut Vec<ClientRequest>) {
        for c in text.chars() {
            key(app, KeyCode::Char(c), out);
        }
    }

    fn draw(app: &mut App) -> String {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, app)).unwrap();
        buffer_text(&terminal)
    }

    /// Click the overlay's spot `hit`, as drawn, through the app's own
    /// mouse handling.
    fn click(app: &mut App, hit: AccountsHit, out: &mut Vec<ClientRequest>) {
        draw(app);
        let rect = view(app)
            .hits
            .iter()
            .find(|(_, h)| *h == hit)
            .map(|(r, _)| *r)
            .unwrap_or_else(|| panic!("{hit:?} not drawn"));
        let event = crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + rect.width / 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        };
        handle_terminal_event(app, crossterm::event::Event::Mouse(event), out);
    }

    fn open_overlay(app: &mut App, out: &mut Vec<ClientRequest>) {
        press(app, KeyCode::Char('U'), KeyModifiers::SHIFT, out);
        assert!(matches!(app.modals.overlay, Some(Overlay::Accounts(_))));
    }

    /// `a`: a harness and a name, checked as `kete profile add` checks
    /// them; the profile is written, its home made, and its sign-in runs
    /// in a new terminal session that gets the login command typed in.
    #[test]
    fn add_writes_the_profile_and_runs_its_sign_in_in_a_new_terminal() {
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            open_overlay(&mut app, &mut out);
            key(&mut app, KeyCode::Char('a'), &mut out);
            let Mode::Form(form) = &view(&app).mode else {
                panic!("no form");
            };
            assert_eq!(form.kind, FormKind::Add);
            assert_eq!(
                form.name,
                format!("{}-2", form.harness),
                "a name is offered"
            );
            let offered = form.name.len();
            assert!(draw(&mut app).contains("Add account"));
            // A bad name is refused where it was typed.
            for _ in 0..offered {
                key(&mut app, KeyCode::Backspace, &mut out);
            }
            typed(&mut app, "bad name", &mut out);
            key(&mut app, KeyCode::Enter, &mut out);
            let Mode::Form(form) = &view(&app).mode else {
                panic!("the form stays up on a refusal");
            };
            assert!(form.error.is_some(), "{form:?}");
            assert!(saved().get("profiles").is_none(), "nothing written");
            for _ in 0.."bad name".len() {
                key(&mut app, KeyCode::Backspace, &mut out);
            }
            // The harness steps with →, back to claude with ←.
            key(&mut app, KeyCode::BackTab, &mut out);
            key(&mut app, KeyCode::Right, &mut out);
            key(&mut app, KeyCode::Left, &mut out);
            key(&mut app, KeyCode::Tab, &mut out);
            typed(&mut app, "work", &mut out);
            click(&mut app, AccountsHit::Key(KeyCode::Enter), &mut out);
            let s = saved();
            assert_eq!(s["profiles"][0]["name"], "work");
            assert_eq!(s["profiles"][0]["harness"], "claude");
            let (config, _) = crate::config::Config::layer_paths();
            assert!(
                config
                    .parent()
                    .unwrap()
                    .join("profiles")
                    .join("work")
                    .exists(),
                "its home is made beside the routed config"
            );
            // The sign-in: a terminal in the selected checkout, then the
            // command typed into it once the daemon made it.
            assert!(
                app.modals.overlay.is_none(),
                "the terminal takes the screen"
            );
            let req_id = match out.as_slice() {
                [ClientRequest::CreateTerminal { req_id, name, .. }] => {
                    assert_eq!(name.as_deref(), Some("sign in work"));
                    *req_id
                }
                other => panic!("expected a CreateTerminal, got {other:?}"),
            };
            out.clear();
            handle_server_event(
                &mut app,
                ServerEvent::Ack {
                    req_id,
                    created: Some(nebula_core::EntityId::Terminal(nebula_core::TerminalId(
                        "t9".into(),
                    ))),
                },
                &mut out,
            );
            assert!(
                out.iter().any(|r| matches!(r,
                    ClientRequest::Input { data, .. } if data == b"kete profile login work\r")),
                "{out:?}"
            );
        });
    }

    /// Adopting the current login makes no home and runs no sign-in; a
    /// name already taken is refused.
    #[test]
    fn add_adopting_the_login_signs_nothing_in_and_a_taken_name_is_refused() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                key(&mut app, KeyCode::Char('a'), &mut out);
                let n = match &view(&app).mode {
                    Mode::Form(f) => f.name.len(),
                    _ => panic!(),
                };
                for _ in 0..n {
                    key(&mut app, KeyCode::Backspace, &mut out);
                }
                typed(&mut app, "work", &mut out);
                key(&mut app, KeyCode::Enter, &mut out);
                assert!(
                    app.chrome.flash.as_deref().unwrap_or("").contains("exists"),
                    "{:?}",
                    app.chrome.flash
                );
                typed(&mut app, "2", &mut out);
                key(&mut app, KeyCode::Tab, &mut out);
                key(&mut app, KeyCode::Char(' '), &mut out);
                key(&mut app, KeyCode::Tab, &mut out);
                key(&mut app, KeyCode::Char(' '), &mut out);
                key(&mut app, KeyCode::Enter, &mut out);
                let s = saved();
                assert_eq!(s["profiles"][1]["name"], "work2");
                assert_eq!(s["profiles"][1]["use_default"], true);
                assert_eq!(s["profiles"][1]["default"], true);
                assert!(out.is_empty(), "no sign-in for an adopted login: {out:?}");
                assert_eq!(view(&app).mode, Mode::List);
                assert_eq!(
                    view(&app).selected().map(|r| r.key.as_str()),
                    Some("work2"),
                    "the cursor lands on the new account"
                );
            },
        );
    }

    /// Enter edits the harness, the kind of login and the variables (the
    /// name and the switch stay); `c` duplicates under a new name.
    #[test]
    fn edit_and_duplicate_write_the_settings() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude","env":{"K":"old"},"disabled":true}]}"#,
            || {
                let mut app = App::new();
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                click(&mut app, AccountsHit::Key(KeyCode::Enter), &mut out);
                let Mode::Form(form) = &view(&app).mode else {
                    panic!("no form");
                };
                assert_eq!(form.env_keys, ["K"]);
                assert!(draw(&mut app).contains("keep K"), "values never shown");
                key(&mut app, KeyCode::Tab, &mut out);
                key(&mut app, KeyCode::Char(' '), &mut out);
                key(&mut app, KeyCode::Tab, &mut out);
                typed(&mut app, "A=1; B=two", &mut out);
                key(&mut app, KeyCode::Enter, &mut out);
                let p = &saved()["profiles"][0];
                assert_eq!(p["use_default"], true);
                assert_eq!(p["env"], serde_json::json!({"A": "1", "B": "two"}));
                assert_eq!(p["disabled"], true, "the switch stays");

                key(&mut app, KeyCode::Char('c'), &mut out);
                let Mode::Form(form) = &view(&app).mode else {
                    panic!("no form");
                };
                assert_eq!(form.name, "work-2");
                key(&mut app, KeyCode::Enter, &mut out);
                let copy = &saved()["profiles"][1];
                assert_eq!(copy["name"], "work-2");
                assert_eq!(copy["env"]["B"], "two", "the variables come along");
                assert!(copy.get("disabled").is_none(), "but not the switch");
            },
        );
    }

    /// `x` warns of the sessions and repos that use the account before
    /// it goes; `n` keeps it, `y` removes it.
    #[test]
    fn remove_warns_of_the_sessions_and_repos_on_the_account() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"}],"projects":{"/tmp/demo":{"default_profiles":{"claude":"work"}}}}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                app.tree.agents[0].profile = Some("work".into());
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                key(&mut app, KeyCode::Char('x'), &mut out);
                let Mode::Remove { message, .. } = &view(&app).mode else {
                    panic!("no confirm");
                };
                assert!(
                    message.contains("1 session runs under it (agent-1)"),
                    "{message}"
                );
                assert!(message.contains("Default account of demo"), "{message}");
                key(&mut app, KeyCode::Char('n'), &mut out);
                assert_eq!(view(&app).mode, Mode::List);
                assert_eq!(saved()["profiles"][0]["name"], "work", "kept");
                key(&mut app, KeyCode::Delete, &mut out);
                click(&mut app, AccountsHit::Key(KeyCode::Char('y')), &mut out);
                assert_eq!(saved()["profiles"], serde_json::json!([]));
                assert!(view(&app).rows.iter().all(|r| r.key != "work"));
            },
        );
    }

    /// The switch and the star are buttons as well as keys.
    #[test]
    fn toggle_and_default_are_buttons_too() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude","default":true},{"name":"home","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                click(&mut app, AccountsHit::Row(1), &mut out);
                assert_eq!(view(&app).cursor, 1);
                click(&mut app, AccountsHit::Key(KeyCode::Char('d')), &mut out);
                assert_eq!(saved()["profiles"][1]["default"], true);
                click(&mut app, AccountsHit::Key(KeyCode::Char(' ')), &mut out);
                assert_eq!(saved()["profiles"][1]["disabled"], true);
                key(&mut app, KeyCode::Char(' '), &mut out);
                assert!(saved()["profiles"][1].get("disabled").is_none());
            },
        );
    }

    /// `p` makes the account a repo's default for its harness, and again
    /// clears it; the spread is set from the top, by click or `s` / `t`.
    #[test]
    fn use_for_repo_and_the_spread_write_the_settings() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                key(&mut app, KeyCode::Char('p'), &mut out);
                assert!(matches!(view(&app).mode, Mode::UseFor { cursor: 0 }));
                assert!(draw(&mut app).contains("demo"));
                key(&mut app, KeyCode::Enter, &mut out);
                assert_eq!(
                    saved()["projects"]["/tmp/demo"]["default_profiles"]["claude"],
                    "work"
                );
                assert_eq!(
                    app.launcher.project_accounts[std::path::Path::new("/tmp/demo")]["claude"],
                    "work",
                    "launches see it at once"
                );
                click(&mut app, AccountsHit::Project(0), &mut out);
                assert!(saved().get("projects").is_none(), "cleared without a trace");
                key(&mut app, KeyCode::Esc, &mut out);

                click(&mut app, AccountsHit::Spread(2), &mut out);
                assert_eq!(saved()["account_spread"], "least_used");
                assert_eq!(
                    app.launcher.spread.spread,
                    nebula_core::accounts::Spread::LeastUsed
                );
                key(&mut app, KeyCode::Char('s'), &mut out);
                assert_eq!(saved()["account_spread"], "manual");
                click(&mut app, AccountsHit::Key(KeyCode::Char('t')), &mut out);
                assert_eq!(saved()["account_spread_threshold"], "95");
            },
        );
    }

    /// `m` moves the session under the grid's cursor onto the selected
    /// account, through MOVE TO ACCOUNT's confirm; one of another harness
    /// is refused.
    #[test]
    fn m_moves_the_cursors_session_onto_the_account() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"},{"name":"gpt","harness":"codex"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                app.nav.focus = Focus::Sessions;
                app.nav.sel_session = 0;
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                let at = view(&app).rows.iter().position(|r| r.key == "gpt").unwrap();
                click(&mut app, AccountsHit::Row(at), &mut out);
                key(&mut app, KeyCode::Char('m'), &mut out);
                assert!(
                    app.chrome
                        .flash
                        .as_deref()
                        .unwrap_or("")
                        .contains("codex account"),
                    "{:?}",
                    app.chrome.flash
                );
                let at = view(&app)
                    .rows
                    .iter()
                    .position(|r| r.key == "work")
                    .unwrap();
                click(&mut app, AccountsHit::Row(at), &mut out);
                click(&mut app, AccountsHit::Key(KeyCode::Char('m')), &mut out);
                let Some(Overlay::Confirm(confirm)) = &app.modals.overlay else {
                    panic!("{:?}", app.modals.overlay);
                };
                assert!(
                    confirm.message.contains("fresh on work"),
                    "{}",
                    confirm.message
                );
                assert!(out.is_empty());
            },
        );
    }

    /// Every form and confirm draws, in a roomy terminal and a tiny one.
    #[test]
    fn every_mode_draws_at_any_size() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                let text = draw(&mut app);
                for word in ["Spread [s]", "[a] add", "[l] sign in", "[p] use for repo"] {
                    assert!(text.contains(word), "{word}: {text}");
                }
                for (open, back) in [
                    ('a', KeyCode::Esc),
                    ('c', KeyCode::Esc),
                    ('x', KeyCode::Char('n')),
                    ('p', KeyCode::Esc),
                ] {
                    key(&mut app, KeyCode::Char(open), &mut out);
                    for (w, h) in [(120u16, 40u16), (40, 6), (20, 3)] {
                        let mut terminal = ratatui::Terminal::new(TestBackend::new(w, h)).unwrap();
                        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                    }
                    key(&mut app, back, &mut out);
                    assert_eq!(view(&app).mode, Mode::List);
                }
            },
        );
    }

    /// Settings → Project: a **Default account** row per harness with
    /// profiles; Enter steps it through the harness's accounts and back to
    /// none, written under the project's repo path, and a launch sees it.
    #[test]
    fn the_project_tab_sets_a_default_account_per_harness() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"},{"name":"home","harness":"claude"},{"name":"gpt","harness":"codex"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                let tab = crate::config::project_tab();
                let declared = crate::config::tab_settings(tab).len();
                assert_eq!(
                    crate::config::tab_len(tab),
                    declared + 2,
                    "claude and codex"
                );
                let rows = crate::config::settings_rows(tab);
                assert!(rows.contains(&crate::config::SettingsRow::Header(
                    crate::config::PROJECT_ACCOUNTS_HEADER.into()
                )));
                press(&mut app, KeyCode::Char('s'), KeyModifiers::NONE, &mut out);
                let digit = char::from_digit(tab as u32 + 1, 10).unwrap();
                press(&mut app, KeyCode::Char(digit), KeyModifiers::NONE, &mut out);
                let text = draw(&mut app);
                assert!(text.contains("Default account"), "{text}");
                assert!(text.contains("none (the spread picks)"), "{text}");

                let claude =
                    || saved()["projects"]["/tmp/demo"]["default_profiles"]["claude"].clone();
                apply_setting_at(&mut app, tab, declared, 0);
                assert_eq!(claude(), "work");
                apply_setting_at(&mut app, tab, declared, 1);
                assert_eq!(claude(), "home");
                assert_eq!(
                    crate::quick_prompt::project_default(
                        &app,
                        &crate::quick_prompt::QuickTarget::Worktree(WorktreeId("w1".into())),
                        "claude"
                    )
                    .as_deref(),
                    Some("home"),
                    "the launch mirror follows"
                );
                assert!(draw(&mut app).contains("[home]"));
                apply_setting_at(&mut app, tab, declared + 1, 0);
                assert_eq!(
                    saved()["projects"]["/tmp/demo"]["default_profiles"]["codex"],
                    "gpt",
                    "one entry per harness"
                );
                apply_setting_at(&mut app, tab, declared, 1);
                apply_setting_at(&mut app, tab, declared + 1, -1);
                assert!(
                    saved().get("projects").is_none(),
                    "none again, no trace left"
                );
            },
        );
    }

    /// The routed config's profiles root: where the overlay makes homes
    /// under tests.
    fn profiles_root() -> std::path::PathBuf {
        let (config, _) = crate::config::Config::layer_paths();
        config.parent().unwrap().join("profiles")
    }

    /// The add form takes variables; an edit's Delete clears them, which
    /// an empty field alone never does.
    #[test]
    fn add_takes_variables_and_edit_clears_them_with_delete() {
        with_config_json("{}", || {
            let mut app = App::new();
            let mut out = Vec::new();
            open_overlay(&mut app, &mut out);
            key(&mut app, KeyCode::Char('a'), &mut out);
            for _ in 0..3 {
                key(&mut app, KeyCode::Tab, &mut out);
            }
            let Mode::Form(form) = &view(&app).mode else {
                panic!("no form");
            };
            assert_eq!(form.current(), Field::Env);
            typed(&mut app, "K=v; L=w", &mut out);
            key(&mut app, KeyCode::Enter, &mut out);
            let s = saved();
            assert_eq!(
                s["profiles"][0]["env"],
                serde_json::json!({"K": "v", "L": "w"})
            );

            key(&mut app, KeyCode::Enter, &mut out);
            assert!(draw(&mut app).contains("[Del] clear variables"));
            key(&mut app, KeyCode::Enter, &mut out);
            assert_eq!(
                saved()["profiles"][0]["env"],
                serde_json::json!({"K": "v", "L": "w"}),
                "an empty field keeps them"
            );
            key(&mut app, KeyCode::Enter, &mut out);
            click(&mut app, AccountsHit::Key(KeyCode::Delete), &mut out);
            assert!(draw(&mut app).contains("clear: drops K, L"));
            key(&mut app, KeyCode::Enter, &mut out);
            assert!(saved()["profiles"][0].get("env").is_none(), "cleared");
        });
    }

    /// `r` renames: refused while a session runs on the account, then the
    /// home, the repo's default and the cursor follow the new name.
    #[test]
    fn rename_moves_the_home_and_waits_for_the_sessions_on_it() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"}],"projects":{"/tmp/demo":{"default_profiles":{"claude":"work"}}}}"#,
            || {
                std::fs::create_dir_all(profiles_root().join("work").join("claude")).unwrap();
                let mut app = App::new();
                seed_tree(&mut app);
                app.tree.agents[0].profile = Some("work".into());
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                // Offline, the sessions on it are unknown: no rename.
                key(&mut app, KeyCode::Char('r'), &mut out);
                assert_eq!(view(&app).mode, Mode::List);
                let flash = app.chrome.flash.clone().unwrap_or_default();
                assert!(flash.contains("not connected"), "{flash}");
                app.chrome.conn = crate::app::ConnState::Connected;
                key(&mut app, KeyCode::Char('r'), &mut out);
                assert_eq!(view(&app).mode, Mode::List, "refused up front");
                let flash = app.chrome.flash.clone().unwrap_or_default();
                assert!(flash.contains("has sessions"), "{flash}");

                app.tree.agents[0].profile = None;
                click(&mut app, AccountsHit::Key(KeyCode::Char('r')), &mut out);
                assert!(draw(&mut app).contains("Rename work"));
                for _ in 0.."work".len() {
                    key(&mut app, KeyCode::Backspace, &mut out);
                }
                typed(&mut app, "max2", &mut out);
                key(&mut app, KeyCode::Enter, &mut out);
                let s = saved();
                assert_eq!(s["profiles"][0]["name"], "max2");
                assert_eq!(
                    s["projects"]["/tmp/demo"]["default_profiles"]["claude"],
                    "max2"
                );
                assert!(profiles_root().join("max2").join("claude").is_dir());
                assert!(!profiles_root().join("work").exists());
                assert_eq!(view(&app).mode, Mode::List);
                assert_eq!(view(&app).selected().map(|r| r.key.as_str()), Some("max2"));
            },
        );
    }

    /// On a daemon whose sessions follow a rename (protocol 60), a session
    /// on the account no longer refuses it: the rename goes through and
    /// the daemon is asked to move the sessions' stored name.
    #[test]
    fn rename_with_sessions_on_it_asks_a_new_daemon_to_follow() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                app.tree.agents[0].profile = Some("work".into());
                app.chrome.conn = crate::app::ConnState::Connected;
                app.orch.daemon_protocol = Some(nebula_core::RENAME_PROFILE_PROTOCOL);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                key(&mut app, KeyCode::Char('r'), &mut out);
                assert!(matches!(view(&app).mode, Mode::Form(_)), "not refused");
                for _ in 0.."work".len() {
                    key(&mut app, KeyCode::Backspace, &mut out);
                }
                typed(&mut app, "max2", &mut out);
                out.clear();
                key(&mut app, KeyCode::Enter, &mut out);
                assert_eq!(saved()["profiles"][0]["name"], "max2");
                assert!(
                    out.iter().any(|r| matches!(
                        r,
                        ClientRequest::RenameProfileSessions { old, new, .. }
                            if old == "work" && new == "max2"
                    )),
                    "{out:?}"
                );
                let flash = app.chrome.flash.clone().unwrap_or_default();
                assert!(flash.contains("1 session follows"), "{flash}");
            },
        );
    }

    /// The remove's `p` deletes the home with the profile; with no home
    /// on disk there is nothing for it to do.
    #[test]
    fn remove_with_p_deletes_the_home_too() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"},{"name":"bare","harness":"claude"}]}"#,
            || {
                std::fs::create_dir_all(profiles_root().join("work").join("claude")).unwrap();
                let mut app = App::new();
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                key(&mut app, KeyCode::Char('x'), &mut out);
                let Mode::Remove { message, home, .. } = &view(&app).mode else {
                    panic!("no confirm");
                };
                assert!(message.contains("p deletes it"), "{message}");
                assert_eq!(
                    home.as_deref(),
                    Some(profiles_root().join("work").as_path())
                );
                click(&mut app, AccountsHit::Key(KeyCode::Char('p')), &mut out);
                assert!(!profiles_root().join("work").exists(), "purged");
                assert_eq!(saved()["profiles"][0]["name"], "bare");

                key(&mut app, KeyCode::Char('x'), &mut out);
                key(&mut app, KeyCode::Char('p'), &mut out);
                assert!(
                    matches!(view(&app).mode, Mode::Remove { home: None, .. }),
                    "no home, no purge"
                );
                assert!(!draw(&mut app).contains("delete its home"));
                key(&mut app, KeyCode::Char('y'), &mut out);
                assert_eq!(saved()["profiles"], serde_json::json!([]));
            },
        );
    }

    // ---- the Wake view ----

    fn view_mut(app: &mut App) -> &mut AccountsView {
        match &mut app.modals.overlay {
            Some(Overlay::Accounts(v)) => v,
            other => panic!("the accounts overlay is not up: {other:?}"),
        }
    }

    /// The overlay turned to its Wake view, a fixed status standing in for
    /// the read: a unit test starts no `kete`, saves nothing, and has no
    /// loop to answer a thread.
    fn open_wake(app: &mut App, out: &mut Vec<ClientRequest>) {
        open_overlay(app, out);
        key(app, KeyCode::Tab, out);
        let status =
            crate::wake_tab::parse_status(crate::wake_tab::fixture::status_json().as_bytes())
                .unwrap();
        let v = view_mut(app);
        assert_eq!(v.tab, Tab::Wake);
        v.wake.status = Some(status);
        v.wake.loading = false;
        assert!(
            app.jobs.view_jobs.is_none(),
            "no thread, no save, no kete under test"
        );
    }

    fn draw_at(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, app)).unwrap();
        buffer_text(&terminal)
    }

    fn anchor(app: &App) -> u16 {
        view(app).wake.selected().unwrap().schedule.anchor()
    }

    /// Two accounts at the two sizes the 0.54 visual pass walks: the
    /// switch, the hour, the next run, the strip with its marks, the last
    /// decision and what stops the heartbeat, in words that are not cut.
    #[test]
    fn the_wake_view_draws_both_accounts_at_80x24_and_120x40() {
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            open_wake(&mut app, &mut out);
            for (w, h) in [(80u16, 24u16), (120, 40)] {
                let text = draw_at(&mut app, w, h);
                // The box's own columns: the screen's rows run on past it,
                // into whatever the launcher draws behind.
                let area = view(&app).area;
                let inside: Vec<String> = text
                    .lines()
                    .map(|l| {
                        l.chars()
                            .skip(usize::from(area.x))
                            .take(usize::from(area.width))
                            .collect()
                    })
                    .collect();
                for want in [
                    "Accounts",
                    "Wake",
                    "work",
                    "home",
                    "[x] on",
                    "[ ] off",
                    "\u{2039} 05:30 \u{203a}",
                    "\u{2039} 05:00 \u{203a}",
                    "every day",
                    "next Sat 05:30",
                    "last 05:32 fired \u{b7} window to 10:32",
                    "no wake-ups yet",
                    "not logged in: kete profile login home",
                    "usual 09:00\u{2013}17:00",
                    "one tiny prompt with your own subscription",
                    "Task Scheduler: Kete\\Whakaoho",
                    "[Space] on/off",
                    "[w] wake now",
                    "[Tab] accounts",
                ] {
                    assert!(text.contains(want), "{w}x{h}: {want}\n{text}");
                }
                // The strip: windows as blocks, now and the hour marked.
                assert!(
                    inside.iter().any(|l| l.matches('\u{2580}').count() >= 8
                        && l.contains('\u{2503}')
                        && l.contains('\u{25c6}')),
                    "{w}x{h}: no strip\n{text}"
                );
                assert!(text.contains("00"), "the scale: {text}");
                // Nothing the user reads is cut with an ellipsis.
                for line in inside.iter().filter(|l| {
                    l.contains("05:30")
                        || l.contains("fired")
                        || l.contains("[ ] off")
                        || l.contains("not logged in")
                }) {
                    assert!(!line.contains('\u{2026}'), "{w}x{h}: cut: {line}");
                }
                assert_eq!(
                    text.contains("suggested 05:30"),
                    w >= 120,
                    "{w}x{h}: {text}"
                );
                // Every spot to click lies inside the box.
                let v = view(&app);
                assert!(
                    v.area.right() <= w && v.area.bottom() <= h,
                    "{w}x{h}: {:?}",
                    v.area
                );
                assert!(v
                    .hits
                    .iter()
                    .any(|(_, h)| *h == AccountsHit::Tab(Tab::Accounts)));
                assert!(v
                    .hits
                    .iter()
                    .any(|(_, h)| *h == AccountsHit::Tab(Tab::Wake)));
                assert!(v.hits.iter().any(|(_, h)| *h == AccountsHit::WakeSwitch(1)));
                for (rect, hit) in &v.hits {
                    assert!(
                        rect.x >= v.area.x
                            && rect.y >= v.area.y
                            && rect.right() <= v.area.right()
                            && rect.bottom() <= v.area.bottom(),
                        "{w}x{h}: {hit:?} {rect:?} outside {:?}",
                        v.area
                    );
                }
            }
            // A terminal too small for it still draws.
            for (w, h) in [(40u16, 12u16), (20, 3)] {
                draw_at(&mut app, w, h);
            }
        });
    }

    /// Before anything lands the view says it is reading; a read that
    /// failed says why. Both keep the keys.
    #[test]
    fn the_wake_view_says_when_it_is_reading_or_could_not() {
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            open_overlay(&mut app, &mut out);
            key(&mut app, KeyCode::Tab, &mut out);
            assert!(view(&app).wake.loading);
            let text = draw_at(&mut app, 100, 30);
            assert!(text.contains("Reading the wake-up status"), "{text}");
            assert!(text.contains("one tiny prompt"), "the words stay: {text}");
            view_mut(&mut app)
                .wake
                .inbox
                .land(0, Err("couldn't run kete: nope".into()));
            let text = draw_at(&mut app, 100, 30);
            assert!(text.contains("Couldn't read the wake-up status"), "{text}");
            assert!(text.contains("couldn't run kete: nope"), "{text}");
            key(&mut app, KeyCode::Tab, &mut out);
            assert_eq!(view(&app).tab, Tab::Accounts, "Tab still turns back");
        });
    }

    /// ←/→ move the hour a quarter hour, round the clock; Space (or Enter)
    /// flips the switch. The row shows it at once and the save is queued,
    /// one an account; with no loop nothing runs and nothing is written.
    #[test]
    fn arrows_move_the_hour_a_quarter_hour_and_space_flips_the_switch() {
        use crate::wake_tab::Edit;
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            open_wake(&mut app, &mut out);
            assert_eq!(anchor(&app), 330);
            key(&mut app, KeyCode::Right, &mut out);
            assert_eq!(anchor(&app), 345);
            assert!(draw(&mut app).contains("\u{2039} 05:45 \u{203a}"));
            key(&mut app, KeyCode::Left, &mut out);
            key(&mut app, KeyCode::Left, &mut out);
            assert_eq!(anchor(&app), 315);
            let text = draw(&mut app);
            assert!(text.contains("\u{2039} 05:15 \u{203a}"), "{text}");
            assert!(
                text.contains("next Sat 05:15"),
                "the next run follows: {text}"
            );
            // Shift moves an hour.
            press(&mut app, KeyCode::Right, KeyModifiers::SHIFT, &mut out);
            assert_eq!(anchor(&app), 375);
            press(&mut app, KeyCode::Left, KeyModifiers::SHIFT, &mut out);
            assert_eq!(anchor(&app), 315);
            // Round the clock, both ways.
            view_mut(&mut app).wake.status.as_mut().unwrap().accounts[0]
                .schedule
                .anchor_min = 0;
            key(&mut app, KeyCode::Left, &mut out);
            assert_eq!(anchor(&app), 1425);
            assert!(draw(&mut app).contains("\u{2039} 23:45 \u{203a}"));
            key(&mut app, KeyCode::Right, &mut out);
            assert_eq!(anchor(&app), 0);
            assert!(draw(&mut app).contains("\u{2039} 00:00 \u{203a}"));
            // The switch.
            view_mut(&mut app).wake.status.as_mut().unwrap().accounts[0]
                .schedule
                .anchor_min = 315;
            key(&mut app, KeyCode::Char(' '), &mut out);
            assert!(!view(&app).wake.selected().unwrap().schedule.enabled);
            assert_eq!(draw(&mut app).matches("[ ] off").count(), 2);
            assert!(!draw(&mut app).contains("next Sat"), "off: nothing next");
            key(&mut app, KeyCode::Enter, &mut out);
            assert!(view(&app).wake.selected().unwrap().schedule.enabled);
            assert_eq!(draw(&mut app).matches("[x] on").count(), 1);
            // One save queued an account, the latest of each.
            key(&mut app, KeyCode::Char(' '), &mut out);
            assert_eq!(
                view(&app).wake.inbox.pending(),
                vec![(
                    "work".to_string(),
                    Edit {
                        enabled: false,
                        anchor_min: 315
                    }
                )]
            );
            key(&mut app, KeyCode::Down, &mut out);
            assert_eq!(view(&app).wake.cursor, 1);
            key(&mut app, KeyCode::Char(' '), &mut out);
            key(&mut app, KeyCode::Right, &mut out);
            assert_eq!(anchor(&app), 315);
            assert_eq!(
                view(&app).wake.inbox.pending()[0],
                (
                    "home".to_string(),
                    Edit {
                        enabled: true,
                        anchor_min: 315
                    }
                )
            );
            // s snaps to the suggestion where there is one.
            key(&mut app, KeyCode::Char('s'), &mut out);
            assert!(
                app.chrome
                    .flash
                    .as_deref()
                    .unwrap_or("")
                    .contains("no suggested hour"),
                "{:?}",
                app.chrome.flash
            );
            key(&mut app, KeyCode::Up, &mut out);
            key(&mut app, KeyCode::Char('s'), &mut out);
            assert_eq!(anchor(&app), 330);
            assert!(out.is_empty(), "nothing goes to the daemon: {out:?}");
            // Esc closes; the queued saves belong to the threads, not the view.
            key(&mut app, KeyCode::Esc, &mut out);
            assert!(app.modals.overlay.is_none());
        });
    }

    /// A click on a tab turns the view, on a row picks it, on a switch
    /// flips it, on an arrow moves the hour; the keys' buttons press their
    /// keys.
    #[test]
    fn clicks_turn_the_tab_pick_a_row_flip_a_switch_and_move_the_hour() {
        with_config_json("{}", || {
            let mut app = App::new();
            seed_tree(&mut app);
            let mut out = Vec::new();
            open_wake(&mut app, &mut out);
            click(&mut app, AccountsHit::WakeRow(1), &mut out);
            assert_eq!(view(&app).wake.cursor, 1);
            click(&mut app, AccountsHit::WakeSwitch(0), &mut out);
            assert_eq!(view(&app).wake.cursor, 0, "the click picked the row too");
            assert!(!view(&app).wake.rows()[0].schedule.enabled);
            click(&mut app, AccountsHit::WakeAnchor(0, 1), &mut out);
            assert_eq!(anchor(&app), 345);
            click(&mut app, AccountsHit::WakeAnchor(0, -1), &mut out);
            click(&mut app, AccountsHit::WakeAnchor(0, -1), &mut out);
            assert_eq!(anchor(&app), 315);
            click(&mut app, AccountsHit::Key(KeyCode::Char('s')), &mut out);
            assert_eq!(anchor(&app), 330, "the suggested button");
            click(&mut app, AccountsHit::Key(KeyCode::Right), &mut out);
            assert_eq!(anchor(&app), 345, "the later button");
            // Tabs, there and back.
            click(&mut app, AccountsHit::Tab(Tab::Accounts), &mut out);
            assert_eq!(view(&app).tab, Tab::Accounts);
            let text = draw(&mut app);
            assert!(
                text.contains("Spread [s]") && text.contains("[Tab] wake-up"),
                "{text}"
            );
            click(&mut app, AccountsHit::Key(KeyCode::Tab), &mut out);
            assert_eq!(view(&app).tab, Tab::Wake, "the Accounts view's Tab button");
            assert_eq!(anchor(&app), 345, "what was edited is still there");
            click(&mut app, AccountsHit::Key(KeyCode::Tab), &mut out);
            assert_eq!(view(&app).tab, Tab::Accounts, "the Wake view's Tab button");
            // The wheel walks the Wake rows when that is the view, over the box.
            key(&mut app, KeyCode::Tab, &mut out);
            draw(&mut app);
            let area = view(&app).area;
            let wheel = |app: &mut App, kind: MouseEventKind, out: &mut Vec<ClientRequest>| {
                let event = crossterm::event::MouseEvent {
                    kind,
                    column: area.x + area.width / 2,
                    row: area.y + area.height / 2,
                    modifiers: KeyModifiers::NONE,
                };
                handle_terminal_event(app, crossterm::event::Event::Mouse(event), out);
            };
            wheel(&mut app, MouseEventKind::ScrollDown, &mut out);
            assert_eq!(view(&app).wake.cursor, 1);
            wheel(&mut app, MouseEventKind::ScrollUp, &mut out);
            assert_eq!(view(&app).wake.cursor, 0);
            assert!(out.is_empty());
        });
    }

    /// The Wake view keeps the cursor on the account the Accounts view had
    /// picked, and the Accounts view follows it back.
    #[test]
    fn the_two_views_share_the_picked_account() {
        with_config_json(
            r#"{"profiles":[{"name":"home","harness":"claude"},{"name":"work","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                let at = view(&app)
                    .rows
                    .iter()
                    .position(|r| r.key == "work")
                    .unwrap();
                click(&mut app, AccountsHit::Row(at), &mut out);
                key(&mut app, KeyCode::Tab, &mut out);
                assert_eq!(view(&app).wake.focus.as_deref(), Some("work"));
                // The status lands: the cursor goes to the account asked for.
                let status = crate::wake_tab::parse_status(
                    crate::wake_tab::fixture::status_json().as_bytes(),
                )
                .unwrap();
                view_mut(&mut app).wake.inbox.land(0, Ok(status));
                draw(&mut app);
                assert_eq!(view(&app).wake.selected_key().as_deref(), Some("work"));
                key(&mut app, KeyCode::Down, &mut out);
                assert_eq!(view(&app).wake.selected_key().as_deref(), Some("home"));
                key(&mut app, KeyCode::Tab, &mut out);
                assert_eq!(view(&app).selected().map(|r| r.key.as_str()), Some("home"));
            },
        );
    }

    /// `w` wakes the picked Claude account (the footer says so at once);
    /// another harness's account is refused in words; the Wake view with
    /// nothing read yet has none to wake. With no loop no thread starts, so
    /// a second press is not held back.
    #[test]
    fn w_wakes_the_picked_claude_account_and_refuses_the_others() {
        with_config_json(
            r#"{"profiles":[{"name":"gpt","harness":"codex"},{"name":"work","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                let at = |app: &App, key: &str| {
                    view(app).rows.iter().position(|r| r.key == key).unwrap()
                };
                let gpt = at(&app, "gpt");
                view_mut(&mut app).cursor = gpt;
                key(&mut app, KeyCode::Char('w'), &mut out);
                let flash = app.chrome.flash.clone().unwrap_or_default();
                assert!(flash.contains("Claude accounts only"), "{flash}");
                let work = at(&app, "work");
                view_mut(&mut app).cursor = work;
                // Not signed in yet: the session would sit at Claude's
                // login until it timed out, so it never starts.
                view_mut(&mut app).rows[work].login = nebula_core::accounts::LoginState::NeedsLogin;
                key(&mut app, KeyCode::Char('w'), &mut out);
                let flash = app.chrome.flash.clone().unwrap_or_default();
                assert!(
                    flash.contains("log work in first: kete profile login work"),
                    "{flash}"
                );
                view_mut(&mut app).rows[work].login = nebula_core::accounts::LoginState::LoggedIn;
                key(&mut app, KeyCode::Char('w'), &mut out);
                assert_eq!(app.chrome.flash.as_deref(), Some("waking work\u{2026}"));
                app.chrome.flash = None;
                click(&mut app, AccountsHit::Key(KeyCode::Char('w')), &mut out);
                assert_eq!(app.chrome.flash.as_deref(), Some("waking work\u{2026}"));
                // The Wake view, before a status has landed.
                key(&mut app, KeyCode::Tab, &mut out);
                key(&mut app, KeyCode::Char('w'), &mut out);
                let flash = app.chrome.flash.clone().unwrap_or_default();
                assert!(flash.contains("still loading"), "{flash}");
                assert!(out.is_empty());
            },
        );
    }

    /// A reserved account says `(yours)` on its own row and the button names
    /// what `y` does next, at both sizes the visual pass walks; the others
    /// carry no mark.
    #[test]
    fn a_reserved_account_is_marked_yours_at_80x24_and_120x40() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude","reserved":true},{"name":"home","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                assert!(view(&app).rows[0].reserved() && !view(&app).rows[1].reserved());
                for (w, h) in [(80u16, 24u16), (120, 40)] {
                    let text = draw_at(&mut app, w, h);
                    let area = view(&app).area;
                    let inside: Vec<String> = text
                        .lines()
                        .map(|l| {
                            l.chars()
                                .skip(usize::from(area.x))
                                .take(usize::from(area.width))
                                .collect()
                        })
                        .collect();
                    for want in ["Accounts", "Spread [s]", "[a] add", "[y] stop reserving"] {
                        assert!(text.contains(want), "{w}x{h}: {want}\n{text}");
                    }
                    let yours: Vec<&String> =
                        inside.iter().filter(|l| l.contains("(yours)")).collect();
                    assert_eq!(yours.len(), 1, "{w}x{h}: {text}");
                    assert!(yours[0].contains("work"), "{w}x{h}: {}", yours[0]);
                    // Under the cursor, the unreserved account offers the key.
                    view_mut(&mut app).cursor = 1;
                    let text = draw_at(&mut app, w, h);
                    assert!(text.contains("[y] reserve for me"), "{w}x{h}: {text}");
                    view_mut(&mut app).cursor = 0;
                }
            },
        );
    }

    /// `y` and a click on its button both reach `toggle_reserved`: the file
    /// takes the flag, then loses the key again; an own login is refused in
    /// words and writes nothing.
    #[test]
    fn y_and_its_button_toggle_reserved() {
        with_config_json(
            r#"{"profiles":[{"name":"work","harness":"claude"},{"name":"home","harness":"claude"}]}"#,
            || {
                let mut app = App::new();
                seed_tree(&mut app);
                let mut out = Vec::new();
                open_overlay(&mut app, &mut out);
                click(&mut app, AccountsHit::Row(1), &mut out);
                key(&mut app, KeyCode::Char('y'), &mut out);
                assert_eq!(
                    app.chrome.flash.as_deref(),
                    Some("home is reserved: kept for your own sessions")
                );
                assert_eq!(saved()["profiles"][1]["reserved"], true);
                assert!(view(&app).rows[1].reserved());
                click(&mut app, AccountsHit::Key(KeyCode::Char('y')), &mut out);
                assert_eq!(
                    app.chrome.flash.as_deref(),
                    Some("home can be picked by scripts again")
                );
                assert!(saved()["profiles"][1].get("reserved").is_none());
                assert!(!view(&app).rows[1].reserved());
                // Another account's own login is not a profile.
                app.launcher.account_usage = vec![own_login(5.0)];
                app.launcher.usage_pushed = true;
                refresh(&mut app);
                let own = view(&app).rows.len() - 1;
                view_mut(&mut app).cursor = own;
                key(&mut app, KeyCode::Char('y'), &mut out);
                let flash = app.chrome.flash.clone().unwrap_or_default();
                assert!(flash.contains("own login"), "{flash}");
                assert!(saved()["profiles"][0].get("reserved").is_none());
            },
        );
    }
}
