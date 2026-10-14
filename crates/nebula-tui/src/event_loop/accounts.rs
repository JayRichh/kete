//! The ACCOUNTS OVERLAY's keys, and the usage the daemon pushes
//! (`ServerEvent::AccountUsage` / `SessionContexts`). State and rows live
//! in `crate::accounts_overlay`.

use super::*;
use crate::accounts_overlay::{build_rows, AccountsView};

/// The accounts to show: the pushed list, or the kept files when the
/// daemon is too old to push one.
fn readings(app: &mut App) -> Vec<nebula_core::usage::AccountUsage> {
    if !app.launcher.usage_pushed {
        app.launcher.account_usage =
            nebula_core::usage::read_all_in(&nebula_core::profiles::profiles_root());
    }
    app.launcher.account_usage.clone()
}

fn rows(app: &mut App) -> Vec<crate::accounts_overlay::AccountRow> {
    let profiles = crate::config::Config::load().profiles;
    app.launcher.profiles = profiles.clone();
    let usage = readings(app);
    build_rows(&profiles, &usage, nebula_core::accounts::login_state)
}

/// `⇧U`: open the overlay.
pub(crate) fn open(app: &mut App) {
    let rows = rows(app);
    app.modals.overlay = Some(Overlay::Accounts(AccountsView::new(
        rows,
        nebula_core::paths::config_path(),
    )));
    app.chrome.dirty = true;
}

/// Rebuild the open overlay's rows (a push, an edit), keeping the cursor.
pub(crate) fn refresh(app: &mut App) {
    if !matches!(app.modals.overlay, Some(Overlay::Accounts(_))) {
        return;
    }
    let rows = rows(app);
    if let Some(Overlay::Accounts(view)) = &mut app.modals.overlay {
        view.rows = rows;
        view.cursor = view.cursor.min(view.rows.len().saturating_sub(1));
    }
}

/// j/k move, Space/e switch a profile on or off, d/* make it the default,
/// Esc (or q) closes.
pub(crate) fn handle_key(app: &mut App, key: KeyEvent) {
    let Some(Overlay::Accounts(view)) = &mut app.modals.overlay else {
        return;
    };
    let result = match key.code {
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
        KeyCode::Char(' ') | KeyCode::Char('e') => Some(view.toggle_enabled()),
        KeyCode::Char('d') | KeyCode::Char('*') => Some(view.set_default()),
        _ => None,
    };
    match result {
        Some(Ok(message)) => {
            app.chrome.flash = Some(message);
            refresh(app);
        }
        Some(Err(message)) => app.chrome.flash = Some(message),
        None => {}
    }
    app.chrome.dirty = true;
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

#[cfg(test)]
mod tests {
    use super::super::tests::{buffer_text, press, seed_tree, with_config_json};
    use super::*;
    use nebula_core::usage::{
        AccountUsage, ContextUsage, SessionContext, UsageSource, UsageWindow, WindowKind,
    };
    use ratatui::backend::TestBackend;

    fn own_login(percent: f32) -> AccountUsage {
        AccountUsage {
            account: "@claude".into(),
            harness: "claude".into(),
            windows: vec![
                UsageWindow {
                    kind: WindowKind::Session,
                    label: None,
                    percent,
                    resets_at: None,
                },
                UsageWindow {
                    kind: WindowKind::Weekly,
                    label: None,
                    percent: 7.0,
                    resets_at: None,
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
}
