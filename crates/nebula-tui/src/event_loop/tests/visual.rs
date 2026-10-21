//! The 0.48-0.50 screens drawn at the two sizes people run the terminal
//! app at, 80x24 and 120x40: the board with a scope and its hidden counts,
//! the docked review, the ship composer, the merge confirm, the verbs
//! menu, the accounts overlay and the Hotkeys tab. Each names what must
//! stay whole at that size (a title, a key, a count) and that nothing
//! runs past the frame it is drawn in. `KETE_DUMP=1` prints the screens.
//!
//! The 0.60 studio screens are drawn the same way in the dark and the
//! light theme: the studio board, the gauntlet, the state map, the triage
//! row, the renew prompt, a role on a card and a reserved account.

use super::board::{agent, bench, bench_at, feat_repo, select_card, with_open_pr};
use super::*;
use crate::keymap::Action;
use nebula_core::{AgentStatus, WorktreeId};

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

fn draw(app: &mut App, w: u16, h: u16, what: &str) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let text = buffer_text(&terminal);
    if std::env::var_os("KETE_DUMP").is_some() {
        eprintln!("---- {what} at {w}x{h} ----\n{text}");
    }
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    // The status bar drops a hint whole rather than ending on `Enter:`.
    let bar = lines.last().map_or("", |l| l.trim_end());
    assert!(
        !bar.ends_with(':'),
        "a hint cut at the bar's edge, {what} at {w}x{h}: {bar}"
    );
    lines
}

/// `needle` is on one row, whole.
fn shows(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|l| l.contains(needle))
}

/// A box drawn with `╭…╮` / `╰…╯` (or square corners: the overlays'
/// frames): every row
/// between its corners has its right edge where the corner put it, so no
/// text ran over the frame.
fn frames_close(lines: &[String]) -> bool {
    let cols = |l: &str| l.chars().collect::<Vec<char>>();
    for (top, line) in lines.iter().enumerate() {
        let row = cols(line);
        let Some(left) = row.iter().position(|&c| c == '╭' || c == '┌') else {
            continue;
        };
        let Some(right) = row[left..]
            .iter()
            .position(|&c| c == '╮' || c == '┐')
            .map(|r| r + left)
        else {
            continue;
        };
        for below in &lines[top + 1..] {
            let r = cols(below);
            match r.get(left) {
                Some('╰') | Some('└') => break,
                Some('│') | Some('├') => {
                    if !matches!(r.get(right), Some('│') | Some('┤')) {
                        return false;
                    }
                }
                _ => break,
            }
        }
    }
    true
}

/// The dark default and Kete light: what every 0.60 screen is drawn in.
const THEMES: [(&str, crate::theme::Theme); 2] = [
    ("dark", crate::theme::KETE_DARK),
    ("light", crate::theme::KETE_LIGHT),
];

/// [`draw`] in `theme`, and no glyph drawn in its own background colour.
fn draw_in(
    app: &mut App,
    theme: (&str, crate::theme::Theme),
    w: u16,
    h: u16,
    what: &str,
) -> Vec<String> {
    use ratatui::style::Color;
    app.chrome.theme = theme.1;
    let what = format!("{what}, {}", theme.0);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    for cell in terminal.backend().buffer().content() {
        assert!(
            cell.symbol().trim().is_empty() || cell.fg == Color::Reset || cell.fg != cell.bg,
            "{:?} unreadable on its own colour, {what} at {w}x{h}",
            cell.symbol()
        );
    }
    draw(app, w, h, &what)
}

/// Write `files` (path, text) under `root`.
fn put(root: &std::path::Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

const STATE_MAP: &str = r#"{"repo":"demo","built_at":0,"note":null,"nodes":[
  {"id":"dept:audio","kind":"department","label":"Audio","status":"","owner":null,"changed_at":null,"questions":0,"links":[]},
  {"id":"SYS-1","kind":"system","label":"Music","status":"building","owner":null,"changed_at":null,"questions":0,"links":[]}],
  "edges":[{"from":"dept:audio","to":"SYS-1","kind":"owns"}]}"#;

/// `demo` as a studio repo: departments, notes (one to Jay), verdicts with
/// evidence, a state map, and a lead and a runner in audio.
fn studio_bench(dir: &tempfile::TempDir) -> App {
    put(
        dir.path(),
        &[
            (
                ".kete/studio.toml",
                "version = 1\ninbox = \"notes\"\ndepartments = \"DEPTS.md\"\n\
                 signoffs = \"rev/signoffs\"\nevidence = \"rev/evidence\"\n\
                 [kinds]\nsound = \"audio\"\nart = \"art-and-3d\"\n",
            ),
            (
                "DEPTS.md",
                "| Department | Owns |\n|---|---|\n| Audio | sound |\n| Art and 3D | models |\n\
                 | Animation (sub-department of Art and 3D) | rigs |\n",
            ),
            (
                "notes/storm.md",
                "---\nstatus: open\nto: audio\npriority: high\ntitle: Storm bed too loud\n---\n",
            ),
            (
                "notes/ask.md",
                "---\nstatus: open\nto: jay\nabout: Audio\ntitle: Pick the bell\n---\n",
            ),
            (
                "rev/signoffs/art/C02.json",
                r#"{"verdict":"split","by":"second-reviewer","at":"2026-10-19","note":"too soft","tags":["sky"],"evidence":["EV1"]}"#,
            ),
            (
                "rev/signoffs/sound/S01.json",
                r#"{"verdict":"approve","at":"2026-10-20"}"#,
            ),
            ("rev/evidence/EV1/shot.png", "12345"),
            (".kete/state-map.json", STATE_MAP),
        ],
    );
    let mut app = bench();
    app.tree.projects[0].repo_path = dir.path().to_path_buf();
    for (id, card, label) in [("a1", "lead", "lead audio"), ("a2", "runner", "Runner")] {
        let agent = nebula_core::AgentId(id.into());
        app.session_roles.insert(
            agent.clone(),
            nebula_core::studio::wire::SessionRole {
                agent,
                card: card.into(),
                label: label.into(),
                tier: if card == "lead" { 1 } else { 3 },
                department: Some("audio".into()),
                permission: "allowlist".into(),
                context_budget: None,
            },
        );
    }
    app
}

fn p1() -> nebula_core::ProjectId {
    nebula_core::ProjectId("p1".into())
}

/// The studio board, collapsed and with Audio open: its departments, the
/// note to Jay, the lead, and the autonomy keys and Esc on the hint row.
#[test]
fn the_studio_board_draws_whole() {
    with_default_config(|| {
        for theme in THEMES {
            for (w, h) in SIZES {
                let dir = tempfile::tempdir().unwrap();
                let mut app = studio_bench(&dir);
                crate::studio_board::open(&mut app, Some(p1()));
                let lines = draw_in(&mut app, theme, w, h, "studio board");
                for want in [
                    "Studio board · demo",
                    "What needs you",
                    "1 note",
                    "Audio",
                    "Animation",
                    "lead audio",
                    "[ ] autonomy",
                    "Esc close",
                ] {
                    assert!(shows(&lines, want), "{want} at {w}x{h}");
                }
                assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
                if let Some(Overlay::StudioBoard(v)) = &mut app.modals.overlay {
                    v.expanded.insert("audio".into());
                    v.expanded.insert(crate::studio_board::NEEDS_KEY.into());
                }
                let lines = draw_in(&mut app, theme, w, h, "studio board, opened");
                assert!(shows(&lines, "Storm bed too loud"), "the note at {w}x{h}");
                assert!(shows(&lines, "Pick the bell"), "the note to Jay at {w}x{h}");
                assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
            }
        }
    });
}

/// The gauntlet: its kinds as tabs, a verdict with its word, and the
/// detail with its evidence.
#[test]
fn the_gauntlet_draws_whole() {
    with_default_config(|| {
        for theme in THEMES {
            for (w, h) in SIZES {
                let dir = tempfile::tempdir().unwrap();
                let mut app = studio_bench(&dir);
                crate::gauntlet_view::open(&mut app, Some(p1()));
                let lines = draw_in(&mut app, theme, w, h, "gauntlet");
                assert!(shows(&lines, "Gauntlet · demo"), "the title at {w}x{h}");
                assert!(shows(&lines, "art 1"), "a tab at {w}x{h}");
                assert!(shows(&lines, "sound 1"), "a tab at {w}x{h}");
                assert!(shows(&lines, "split"), "the verdict's word at {w}x{h}");
                assert!(shows(&lines, "Esc close"), "Esc at {w}x{h}");
                assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
                crate::gauntlet_view::open_on(&mut app, Some(p1()), "art", "C02");
                let lines = draw_in(&mut app, theme, w, h, "gauntlet detail");
                assert!(shows(&lines, "too soft"), "the note at {w}x{h}");
                assert!(shows(&lines, "shot.png"), "the evidence at {w}x{h}");
                assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
            }
        }
    });
}

/// `s` on the board: the state map, its rows and its keys.
#[test]
fn the_state_map_draws_whole() {
    with_default_config(|| {
        for theme in THEMES {
            for (w, h) in SIZES {
                let dir = tempfile::tempdir().unwrap();
                let mut app = studio_bench(&dir);
                let root = dir.path().to_path_buf();
                crate::state_overlay::open(&mut app, Some(p1()), "demo".into(), root);
                let lines = draw_in(&mut app, theme, w, h, "state map");
                assert!(shows(&lines, "State map of demo"), "the title at {w}x{h}");
                assert!(
                    shows(&lines, "department dept:audio: Audio"),
                    "a row at {w}x{h}"
                );
                assert!(shows(&lines, "esc close"), "Esc at {w}x{h}");
                assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
            }
        }
    });
}

/// The orchestrator view's triage row: the hint, then the open field
/// with its keys.
#[test]
fn the_triage_row_draws_whole() {
    with_default_config(|| {
        for theme in THEMES {
            for (w, h) in SIZES {
                let dir = tempfile::tempdir().unwrap();
                let mut app = studio_bench(&dir);
                agent(&mut app, "a1").role = nebula_core::AgentRole::Orchestrator;
                app.orch.view = true;
                app.pane.collapsed = true;
                app.pane.term = Some(crate::app::AttachedTerm::new(
                    nebula_core::SessionRef::Agent(nebula_core::AgentId("a1".into())),
                    w,
                    h,
                ));
                app.pane.term_locked = false;
                app.nav.focus = crate::app::Focus::Terminal;
                let lines = draw_in(&mut app, theme, w, h, "orchestrator view");
                assert!(shows(&lines, "t triage note"), "the hint at {w}x{h}");
                crate::triage_strip::open(&mut app);
                let lines = draw_in(&mut app, theme, w, h, "triage open");
                assert!(shows(&lines, "triage ›"), "the field at {w}x{h}");
                assert!(shows(&lines, "Esc closes"), "Esc at {w}x{h}");
                // The strip is the thread's: closed for the next size.
                let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
                assert!(crate::triage_strip::handle_key(&mut app, esc));
            }
        }
    });
}

/// Renew orchestrator: the prompt and its keys; Enter on no note closes
/// it, as every prompt does.
#[test]
fn the_renew_prompt_draws_whole() {
    with_default_config(|| {
        for theme in THEMES {
            for (w, h) in SIZES {
                let mut app = bench();
                crate::renew_view::open(&mut app);
                let lines = draw_in(&mut app, theme, w, h, "renew prompt");
                assert!(shows(&lines, "Renew orchestrator"), "the title at {w}x{h}");
                assert!(shows(&lines, "the handover note"), "the label at {w}x{h}");
                assert!(shows(&lines, "[Esc] cancel"), "Esc at {w}x{h}");
                assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
                press(
                    &mut app,
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    &mut Vec::new(),
                );
                let lines = draw_in(&mut app, theme, w, h, "renew, no note");
                assert!(
                    shows(&lines, "cancelled: empty input"),
                    "the flash at {w}x{h}"
                );
            }
        }
    });
}

/// A role session's card leads its runs-on line with the role, whole at
/// 120 and first at 80 where the card is narrow.
#[test]
fn a_role_shows_on_its_card() {
    with_default_config(|| {
        for theme in THEMES {
            for (w, h) in SIZES {
                let dir = tempfile::tempdir().unwrap();
                let mut app = studio_bench(&dir);
                let lines = draw_in(&mut app, theme, w, h, "board with roles");
                let want = if w >= 120 {
                    "Runner · claude"
                } else {
                    "Runner · "
                };
                assert!(shows(&lines, want), "the role at {w}x{h}");
            }
        }
    });
}

/// The accounts overlay with a reserved account: marked yours.
#[test]
fn a_reserved_account_draws_whole() {
    let json = r#"{"profiles":[{"name":"work","harness":"claude","reserved":true},{"name":"home","harness":"claude"}]}"#;
    with_config_json(json, || {
        for theme in THEMES {
            for (w, h) in SIZES {
                let mut app = bench();
                accounts::open(&mut app);
                let lines = draw_in(&mut app, theme, w, h, "accounts, reserved");
                assert!(
                    lines
                        .iter()
                        .any(|l| l.contains("work") && l.contains("(yours)")),
                    "the reserved row at {w}x{h}"
                );
                assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
            }
        }
    });
}

fn scoped_board() -> App {
    let mut app = bench();
    // A running session on the root, outside the scope to come.
    seed_agent_in(&mut app, "a3", &WorktreeId("w1".into()));
    agent(&mut app, "a3").status = AgentStatus::Running;
    agent(&mut app, "a2").profile = Some("work".into());
    app.launcher.scope = nebula_core::scope::Scope {
        repo: Some(nebula_core::ProjectId("p1".into())),
        worktree: Some(WorktreeId("w2".into())),
        ..Default::default()
    };
    app
}

/// The board with a scope: the chip says where, its ✕ stays in view, and
/// each lane's foot counts what the scope leaves out.
#[test]
fn the_scoped_board_draws_whole() {
    with_default_config(|| {
        for (w, h) in SIZES {
            let mut app = scoped_board();
            let lines = draw(&mut app, w, h, "scoped board");
            assert!(shows(&lines, "✕"), "the chip's clear at {w}x{h}");
            assert!(
                shows(&lines, "feat"),
                "the chip names the worktree at {w}x{h}"
            );
            // The count whole, however narrow the lane: never `1 more outsid…`.
            assert!(
                shows(&lines, "1 more outside scope") || shows(&lines, "+1 outside"),
                "the hidden count at {w}x{h}"
            );
            assert!(
                !lines.iter().any(|l| l.contains("outsid…")),
                "a cut count at {w}x{h}"
            );
            assert!(shows(&lines, "polish-"), "the card at {w}x{h}");
        }
    });
}

/// The review unit docked beside the board: its verb row keeps Reply and
/// the PR verb, and the lanes stay beside it.
#[test]
fn the_docked_review_draws_whole() {
    with_default_config(|| {
        for (w, h) in SIZES {
            let dir = tempfile::tempdir().unwrap();
            let mut app = bench_at(feat_repo(&dir));
            note_worktree_changes(&mut app, WorktreeId("w2".into()), Some(2));
            draw(&mut app, w, h, "board");
            select_card(&mut app, "a2");
            press(
                &mut app,
                KeyCode::Enter,
                KeyModifiers::NONE,
                &mut Vec::new(),
            );
            assert!(matches!(&app.modals.overlay, Some(Overlay::Diff(v)) if v.review.is_some()));
            let lines = draw(&mut app, w, h, "docked review");
            assert!(shows(&lines, "reply"), "Reply on the verb row at {w}x{h}");
            assert!(shows(&lines, "PR"), "the PR verb at {w}x{h}");
            // Beside the dock, a lane shows a card's name, or the dock
            // takes the board: never lanes of `● p…`.
            assert!(
                !shows(&lines, "● p…"),
                "lanes too narrow to read at {w}x{h}"
            );
            assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
        }
    });
}

/// The ship composer: its title, the fields and the send key, inside its
/// frame; and its sign-in error, whole.
#[test]
fn the_ship_composer_draws_whole() {
    with_default_config(|| {
        for (w, h) in SIZES {
            let mut app = bench();
            super::super::ship::open_composer(&mut app, &WorktreeId("w2".into()), None, None);
            let lines = draw(&mut app, w, h, "ship composer");
            assert!(shows(&lines, "Pull request: feat"), "the title at {w}x{h}");
            assert!(shows(&lines, "Title"), "the title field at {w}x{h}");
            assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
            if let Some(Overlay::Ship(v)) = &mut app.modals.overlay {
                v.stage = crate::ship_view::Stage::Failed(kete_ado::ship::ShipError {
                    kind: kete_ado::ship::ShipErrorKind::SignIn,
                    forge: Some(kete_ado::ship::ForgeKind::GitHub),
                    message: kete_ado::ship::GH_SIGN_IN.into(),
                    url: None,
                });
            }
            let lines = draw(&mut app, w, h, "ship composer, signed out");
            assert!(shows(&lines, "gh auth login"), "the fix at {w}x{h}");
            assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
        }
    });
}

/// The merge confirm: the number, the methods and the branch choice.
#[test]
fn the_merge_confirm_draws_whole() {
    with_default_config(|| {
        for (w, h) in SIZES {
            let mut app = bench();
            agent(&mut app, "a2").unseen = false;
            with_open_pr(&mut app);
            super::super::ship::open_merge(&mut app, &WorktreeId("w2".into()), None, None);
            let lines = draw(&mut app, w, h, "merge confirm");
            assert!(shows(&lines, "Merge #12: feat"), "the title at {w}x{h}");
            assert!(shows(&lines, "squash"), "the methods at {w}x{h}");
            assert!(shows(&lines, "[Enter] Merge"), "the send key at {w}x{h}");
            assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
        }
    });
}

/// `?` on a card: the verbs menu with its filter line and its keys, and
/// All keys last, in view.
#[test]
fn the_verbs_menu_draws_whole() {
    with_default_config(|| {
        for (w, h) in SIZES {
            let mut app = bench();
            draw(&mut app, w, h, "board");
            select_card(&mut app, "a2");
            run_action(&mut app, Action::Verbs, &mut Vec::new());
            assert!(matches!(app.modals.overlay, Some(Overlay::Menu(_))));
            let lines = draw(&mut app, w, h, "verbs menu");
            assert!(
                shows(&lines, "polish-nav"),
                "the menu names the card at {w}x{h}"
            );
            assert!(shows(&lines, "Archive"), "a verb at {w}x{h}");
            assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
        }
    });
}

/// The accounts overlay: its title and every login's name and state.
#[test]
fn the_accounts_overlay_draws_whole() {
    with_default_config(|| {
        for (w, h) in SIZES {
            let mut app = bench();
            accounts::open(&mut app);
            let lines = draw(&mut app, w, h, "accounts");
            assert!(shows(&lines, "Accounts"), "the title at {w}x{h}");
            assert!(shows(&lines, "claude"), "a login at {w}x{h}");
            // The spread's words whole, or ended on an ellipsis, not cut
            // at the frame.
            assert!(
                shows(&lines, "when none is starred") || shows(&lines, "…│"),
                "the spread's line cut at {w}x{h}"
            );
            assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
        }
    });
}

/// Settings, Hotkeys: the tab strip keeps Hotkeys, a group and a row
/// with its chord.
#[test]
fn the_hotkeys_tab_draws_whole() {
    with_default_config(|| {
        for (w, h) in SIZES {
            let mut app = bench();
            open_settings_on(&mut app, crate::config::hotkeys_tab(), &mut Vec::new());
            let lines = draw(&mut app, w, h, "hotkeys");
            assert!(shows(&lines, "Hotkeys"), "the tab at {w}x{h}");
            assert!(shows(&lines, "NAVIGATE"), "a group at {w}x{h}");
            assert!(frames_close(&lines), "a frame overrun at {w}x{h}");
        }
    });
}
