//! LEARN IN PLACE (`learn.rs`): the first-use tip's state machine — shown
//! once, dismissed, silenced — the WHAT'S NEW trigger on a version change,
//! and the empty states, drawn at a stock 80x24.

use super::board::bench;
use super::*;
use crate::learn::{TipButton, DWELL};
use nebula_config::learn::{TIP_DRAG, TIP_LANES};

fn draw_80x24(app: &mut App) -> String {
    crate::learn::observe(app);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    buffer_text(&terminal)
}

/// The board with a Review card, its tips armed from the (test's) config.
fn armed_bench() -> App {
    let mut app = bench();
    let cfg = crate::config::Config::load();
    crate::learn::arm(&mut app, &cfg, false, true);
    app.modals.overlay = None;
    app
}

fn shown(app: &App) -> Option<&'static str> {
    app.learn.tip.as_ref().map(|t| t.id)
}

#[test]
fn a_tip_shows_once_with_its_buttons_and_is_remembered() {
    with_default_config(|| {
        let mut app = armed_bench();
        let text = draw_80x24(&mut app);
        assert_eq!(shown(&app), Some(TIP_LANES), "a card in Review");
        assert!(text.contains("Review: finished work"), "{text}");
        assert!(
            text.contains("[got it]") && text.contains("[no tips]"),
            "{text}"
        );
        assert!(app.hit_rect(&HitTarget::Tip(TipButton::GotIt)).is_some());
        // Marked seen the moment it showed, in the shared config.
        let cfg = crate::config::Config::load();
        assert!(cfg.tips_seen.contains(TIP_LANES), "{:?}", cfg.tips_seen);

        // Got it: gone, and the next concept on screen gets its turn —
        // never the one already shown.
        crate::learn::click(&mut app, TipButton::GotIt);
        assert_eq!(shown(&app), None);
        draw_80x24(&mut app);
        assert_eq!(shown(&app), Some(TIP_DRAG), "two cards on the board");
        crate::learn::click(&mut app, TipButton::GotIt);

        // A new app over the same config doesn't show either again.
        let mut again = armed_bench();
        draw_80x24(&mut again);
        assert!(
            !matches!(shown(&again), Some(TIP_LANES | TIP_DRAG)),
            "{:?}",
            shown(&again)
        );
    });
}

#[test]
fn one_tip_at_a_time_and_the_waiting_one_is_not_marked() {
    with_default_config(|| {
        let mut app = armed_bench();
        draw_80x24(&mut app);
        assert_eq!(shown(&app), Some(TIP_LANES));
        // Drag is due too, but waits: not shown, not seen.
        draw_80x24(&mut app);
        assert_eq!(shown(&app), Some(TIP_LANES));
        assert!(!app.learn.seen.contains(TIP_DRAG));
        assert!(!crate::config::Config::load().tips_seen.contains(TIP_DRAG));
    });
}

#[test]
fn no_tips_turns_every_tip_off_in_the_config() {
    with_default_config(|| {
        let mut app = armed_bench();
        draw_80x24(&mut app);
        assert!(shown(&app).is_some());
        let rect = app.hit_rect(&HitTarget::Tip(TipButton::Off)).unwrap();
        click(&mut app, rect.x + 1, rect.y, &mut Vec::new());
        assert_eq!(shown(&app), None);
        assert!(!crate::config::Config::load().show_tips);
        let text = draw_80x24(&mut app);
        assert_eq!(shown(&app), None, "off stays off");
        assert!(!text.contains("[got it]"), "{text}");
    });
}

#[test]
fn show_tips_off_in_the_config_shows_none() {
    with_config_json(r#"{"show_tips": false}"#, || {
        let mut app = armed_bench();
        draw_80x24(&mut app);
        assert_eq!(shown(&app), None);
    });
}

#[test]
fn a_key_dismisses_a_tip_only_once_it_has_been_read() {
    with_default_config(|| {
        let mut app = armed_bench();
        draw_80x24(&mut app);
        let since = app.learn.tip.as_ref().unwrap().since;
        crate::learn::on_key(&mut app, since + DWELL / 2);
        assert!(shown(&app).is_some(), "the key that brought it up keeps it");
        crate::learn::on_key(&mut app, since + DWELL);
        assert_eq!(shown(&app), None);
    });
}

#[test]
fn an_unarmed_app_shows_no_tip_and_writes_nothing() {
    with_default_config(|| {
        let mut app = bench();
        draw_80x24(&mut app);
        assert_eq!(shown(&app), None);
        let (path, _) = crate::config::Config::layer_paths();
        assert!(!path.exists(), "nothing written");
    });
}

#[test]
fn whats_new_shows_once_after_an_upgrade() {
    with_config_json(r#"{"whats_new_seen": "0.48"}"#, || {
        let mut app = App::new();
        let cfg = crate::config::Config::load();
        assert!(
            crate::learn::arm(&mut app, &cfg, false, true),
            "0.48 to this"
        );
        let current = nebula_config::learn::current_version();
        assert_eq!(crate::config::Config::load().whats_new_seen, current);
        let cfg = crate::config::Config::load();
        assert!(
            !crate::learn::arm(&mut App::new(), &cfg, false, true),
            "once"
        );
    });
}

#[test]
fn a_first_install_skips_whats_new_and_records_the_release() {
    with_default_config(|| {
        let mut app = App::new();
        let cfg = crate::config::Config::load();
        assert!(!crate::learn::arm(&mut app, &cfg, true, true));
        assert_eq!(
            crate::config::Config::load().whats_new_seen,
            nebula_config::learn::current_version()
        );
    });
}

#[test]
fn whats_new_reads_at_80x24_and_turns_to_the_keys_and_back() {
    with_default_config(|| {
        let mut app = App::new();
        seed_tree(&mut app);
        crate::learn::open_whats_new(&mut app);
        let text = draw_80x24(&mut app);
        assert!(text.contains("What's new"), "{text}");
        let newest = &nebula_config::learn::WHATS_NEW[0];
        assert!(
            text.contains(newest.version) && text.contains(newest.title),
            "{text}"
        );
        assert!(text.contains("Esc: close"), "{text}");
        let mut out = Vec::new();
        for _ in 0..80 {
            press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE, &mut out);
        }
        let text = draw_80x24(&mut app);
        assert!(
            text.contains("Keys changed"),
            "scrolled to the table: {text}"
        );
        press(&mut app, KeyCode::F(1), KeyModifiers::NONE, &mut out);
        let text = draw_80x24(&mut app);
        assert!(text.contains("w: what's new"), "{text}");
        press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE, &mut out);
        assert!(matches!(&app.modals.overlay, Some(Overlay::Help(h)) if h.whats_new));
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE, &mut out);
        assert!(app.modals.overlay.is_none());
    });
}

#[test]
fn the_palette_runs_whats_new() {
    with_default_config(|| {
        let mut app = App::new();
        seed_tree(&mut app);
        run_action(&mut app, crate::keymap::Action::WhatsNew, &mut Vec::new());
        assert!(matches!(&app.modals.overlay, Some(Overlay::Help(h)) if h.whats_new));
    });
}

#[test]
fn an_empty_board_says_what_goes_where_at_80x24() {
    with_default_config(|| {
        let mut app = App::new();
        seed_tree(&mut app);
        app.tree.agents.clear();
        app.launcher.launcher_pane_at = crate::launcher::PaneSide::Bottom;
        app.launcher.launcher_kanban = true;
        app.launcher.kanban_all = true;
        let text = draw_80x24(&mut app);
        assert!(text.contains("No sessions yet"), "{text}");
        // The start key is the user's, from the keymap.
        let key = crate::ui::key_hint(&app, crate::keymap::Action::QuickPrompt);
        assert!(text.contains(&format!("{key}: ")), "{text}");
        // Each empty lane says what lands in it.
        for words in ["Agents at", "Nothing", "Finished", "open PR", "Merged"] {
            assert!(text.contains(words), "{words}: {text}");
        }
    });
}

#[test]
fn an_empty_lane_beside_full_ones_keeps_its_line() {
    with_default_config(|| {
        let mut app = bench();
        let text = draw_80x24(&mut app);
        assert!(text.contains("Agents at"), "Running is empty: {text}");
    });
}

#[test]
fn the_empty_band_names_the_users_own_keys() {
    with_config_json(r#"{"keybindings": {"quick_prompt": "ctrl+y"}}"#, || {
        let mut app = App::new();
        app.chrome.keymap = crate::config::Config::load().keymap();
        let hint = crate::ui::launcher_view::empty_band_hint(&app, false);
        assert!(hint.contains("^y: new session"), "{hint}");
        assert!(!hint.contains("p: new session"), "{hint}");
    });
}

#[test]
fn the_footer_holds_at_most_seven_entries() {
    with_default_config(|| {
        let mut app = bench();
        let mut terminal = Terminal::new(TestBackend::new(400, 24)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        let shown = app
            .chrome
            .hits
            .iter()
            .filter(|(_, h)| matches!(h, HitTarget::FooterHint(_)))
            .count();
        assert!(shown <= crate::ui::footer::FOOTER_MAX, "{shown}");
    });
}

/// A TUI attached to a daemon an older install left running says so on
/// its first snapshot, with how to move onto this build; on a daemon of
/// its own protocol it says nothing.
#[test]
fn an_older_daemon_is_named_on_the_first_snapshot() {
    for (theirs, said) in [
        (nebula_core::PROTOCOL_VERSION - 3, true),
        (nebula_core::PROTOCOL_VERSION, false),
    ] {
        let mut seeded = App::new();
        seed_tree(&mut seeded);
        let tree = seeded.tree.clone();
        let mut app = App::new();
        app.orch.daemon_protocol = Some(theirs);
        hse(
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
        );
        let flash = app.chrome.flash.clone().unwrap_or_default();
        assert_eq!(flash.contains("older build"), said, "{theirs}: {flash:?}");
        if said {
            assert!(flash.contains(if cfg!(windows) {
                "kete kill"
            } else {
                "kete reload"
            }));
        }
    }
}

/// The one-time notices lead with what to run, so a 120-column footer
/// (beside the connection and the breadcrumb) shows the tracked-settings
/// command whole, and what to run on an older daemon.
#[test]
fn the_upgrade_notices_show_their_command_at_120_columns() {
    let found = [(
        "alpha feat/login".to_string(),
        ".claude/settings.local.json",
    )];
    for (flash, needle) in [
        (
            nebula_core::tracked_settings::notice(&found).unwrap(),
            "git rm --cached .claude/settings.local.json",
        ),
        (
            crate::event_loop::background::OLDER_DAEMON.to_string(),
            if cfg!(windows) {
                "kete kill"
            } else {
                "kete reload"
            },
        ),
    ] {
        let mut app = App::new();
        seed_tree(&mut app);
        app.chrome.flash = Some(flash);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        let screen = buffer_text(&terminal);
        assert!(
            screen.contains(needle),
            "{needle}:
{screen}"
        );
    }
}

/// The page opens before the daemon's first snapshot lands, and is
/// already recorded as shown: the snapshot (its remembered selection and
/// pane) must not close it, or the user never sees it.
#[test]
fn whats_new_survives_the_first_snapshot() {
    with_config_json(r#"{"whats_new_seen": "0.48"}"#, || {
        let mut seeded = App::new();
        seed_tree(&mut seeded);
        let tree = seeded.tree.clone();
        let mut app = App::new();
        let cfg = crate::config::Config::load();
        assert!(crate::learn::arm(&mut app, &cfg, false, true));
        crate::learn::open_whats_new(&mut app);
        hse(
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
        );
        draw_80x24(&mut app);
        assert!(
            matches!(&app.modals.overlay, Some(Overlay::Help(h)) if h.whats_new),
            "{:?}",
            app.modals.overlay.as_ref().map(|_| "another overlay")
        );
        assert!(app.learn.tip.is_none(), "no tip over the page");
    });
}
