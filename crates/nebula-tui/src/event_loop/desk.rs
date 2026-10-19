//! Keys, clicks and daemon events of the views ported from the desktop
//! window: the SUBAGENTS OVERLAY and its chips (`crate::subagents`), the
//! COMMAND PALETTE (`^K`, the `/` palette with every action in it), the
//! ATTENTION STRIP (`crate::attention`) and BACK / FORWARD
//! (`crate::history`). Their state is `App::desk`; their drawing
//! `ui::subagents_view` and `ui::attention_view`.

use super::*;
use crate::history::Place;
use crate::subagents::{build_forest, Node, SubagentsView, Tone};
use nebula_core::SubagentSummary;

// ---- subagents ----

/// A snapshot's subagents: the whole book again.
pub(crate) fn on_snapshot_subagents(app: &mut App, subs: Vec<SubagentSummary>) {
    app.desk.subagents.replace(subs);
    refresh_subagents(app);
    // And which of the projects are on Azure DevOps, for the header's
    // `ado` button.
    super::ado::probe(app);
}

/// One subagent started, ended, or closed with its session.
pub(crate) fn on_subagent_changed(app: &mut App, sub: SubagentSummary) {
    app.desk.subagents.upsert(sub);
    refresh_subagents(app);
    app.chrome.dirty = true;
}

/// The daemon dropped some of `parent`'s subagents.
pub(crate) fn on_subagents_removed(app: &mut App, parent: &AgentId, ids: &[String]) {
    app.desk.subagents.remove(parent, ids);
    refresh_subagents(app);
    app.chrome.dirty = true;
}

/// Can the daemon take `ClearEndedSubagents`? None (no handshake yet:
/// tests) counts as yes, as `account_switch::daemon_switches` does.
fn daemon_clears_subagents(app: &App) -> bool {
    app.orch
        .daemon_protocol
        .is_none_or(|v| v >= nebula_core::SUBAGENT_CLEANUP_PROTOCOL)
}

/// `x` in the SUBAGENTS OVERLAY (`X`: every session): ask the daemon to
/// drop the ended subagents of the cursor row's session. Running ones
/// stay; the rows leave as the daemon's `SubagentsRemoved` comes back.
pub(crate) fn clear_finished_subagents(
    app: &mut App,
    every_session: bool,
    out: &mut Vec<ClientRequest>,
) {
    let parent = if every_session {
        None
    } else {
        let Some(Overlay::Subagents(view)) = &app.modals.overlay else {
            return;
        };
        let Some(row) = view.rows.get(view.cursor) else {
            return;
        };
        Some(row.node.opens.clone())
    };
    let ended = match &parent {
        Some(p) => app.desk.subagents.ended(p),
        None => app.desk.subagents.ended_total(),
    };
    if ended == 0 {
        app.chrome.flash = Some("no finished subagents to clear".into());
        return;
    }
    if !daemon_clears_subagents(app) {
        app.chrome.flash = Some(
            "the running daemon predates clearing subagents: `kete reload` moves it onto this build"
                .into(),
        );
        return;
    }
    let req_id = app.alloc_req_id(PendingIntent::None);
    out.push(ClientRequest::ClearEndedSubagents { req_id, parent });
    app.chrome.flash = Some(format!(
        "clearing {ended} finished subagent{}",
        if ended == 1 { "" } else { "s" }
    ));
}

/// `d` in the SUBAGENTS OVERLAY: flip the `hide_done_subagents` SETTING
/// and write it back, so the cards and the window's board follow too.
fn toggle_hide_done_subagents(app: &mut App) {
    let mut cfg = crate::config::Config::load();
    cfg.hide_done_subagents = !app.desk.subagents.hide_done;
    if !super::settings_handlers::save_config(app, &cfg) {
        return;
    }
    app.desk.subagents.hide_done = cfg.hide_done_subagents;
    refresh_subagents(app);
    app.chrome.flash = Some(if cfg.hide_done_subagents {
        "finished subagents folded under their session (Settings → Sessions)".into()
    } else {
        "finished subagents shown one by one".into()
    });
}

/// The SESSION TREE of `project`'s live sessions — every project's, one
/// after another, for None.
pub(crate) fn forest_of(app: &App, project: Option<&ProjectId>) -> Vec<Node> {
    app.tree
        .projects
        .iter()
        .filter(|p| project.is_none_or(|id| &p.id == id))
        .flat_map(|p| {
            let agents: Vec<&nebula_core::Agent> = app
                .tree
                .agents
                .iter()
                .filter(|a| !a.archived)
                .filter(|a| {
                    app.tree
                        .worktrees
                        .iter()
                        .any(|w| w.id == a.worktree_id && w.project_id == p.id)
                })
                .collect();
            build_forest(&agents, &app.desk.subagents, app.desk.subagents.hide_done)
        })
        .collect()
}

/// `⇧S`, or a click on a `⑂` chip (`on`): the SUBAGENTS OVERLAY on the
/// project in front of you, its cursor on `on` — else on the card under
/// the grid's cursor — with that session's subagents unfolded.
pub(crate) fn open_subagents(app: &mut App, on: Option<AgentId>) {
    let project = app.selected_project().map(|p| p.id.clone());
    let on = on.or_else(|| match app.selected_session_row() {
        Some(SessionRow::Agent(a)) => Some(a.id),
        _ => None,
    });
    // A session from another project (a chip on the board's every-project
    // lanes) opens that project's tree.
    let project = on
        .as_ref()
        .and_then(|id| app.tree.agents.iter().find(|a| &a.id == id))
        .and_then(|a| app.tree.worktrees.iter().find(|w| w.id == a.worktree_id))
        .map(|w| w.project_id.clone())
        .or(project);
    let mut view = SubagentsView {
        project,
        ..SubagentsView::default()
    };
    let forest = forest_of(app, view.project.as_ref());
    if let Some(id) = &on {
        let key = format!("a:{}", id.0);
        view.open.insert(key.clone());
        view.rebuild(&forest);
        view.land_on(&key);
    } else {
        view.rebuild(&forest);
    }
    app.modals.overlay = Some(Overlay::Subagents(view));
    app.chrome.dirty = true;
}

/// Rebuild the open overlay's rows after the tree or the book changed.
pub(crate) fn refresh_subagents(app: &mut App) {
    let Some(Overlay::Subagents(view)) = &app.modals.overlay else {
        return;
    };
    let forest = forest_of(app, view.project.clone().as_ref());
    if let Some(Overlay::Subagents(view)) = &mut app.modals.overlay {
        view.rebuild(&forest);
    }
}

/// The session the overlay's cursor row opens, the overlay put away:
/// the palette's landing, so a session waiting on you attaches.
fn open_row(app: &mut App, out: &mut Vec<ClientRequest>) {
    let Some(Overlay::Subagents(view)) = &app.modals.overlay else {
        return;
    };
    let Some(id) = view.rows.get(view.cursor).map(|r| r.node.opens.clone()) else {
        return;
    };
    app.modals.overlay = None;
    let target = PaletteTarget::Session(id);
    let attaches = crate::config::Config::load().palette_enter_attaches;
    let landing = Landing::for_enter_on(app, &target, attaches);
    jump_to_target(app, target, landing, out);
}

/// j/k walk, → / l open a row's children, ← / h fold them (or step to
/// the row above it), Space flips, Tab this project or every project,
/// Enter opens the session, Esc / q close.
pub(crate) fn handle_subagents_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) {
    if matches!(key.code, KeyCode::Enter) {
        open_row(app, out);
        return;
    }
    match key.code {
        KeyCode::Char('d') => {
            toggle_hide_done_subagents(app);
            app.chrome.dirty = true;
            return;
        }
        KeyCode::Char(c @ ('x' | 'X')) => {
            clear_finished_subagents(app, c == 'X', out);
            app.chrome.dirty = true;
            return;
        }
        _ => {}
    }
    if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        let current = app.selected_project().map(|p| p.id.clone());
        if let Some(Overlay::Subagents(view)) = &mut app.modals.overlay {
            view.project = if view.project.is_some() {
                None
            } else {
                current
            };
        }
        refresh_subagents(app);
        app.chrome.dirty = true;
        return;
    }
    let Some(Overlay::Subagents(view)) = &mut app.modals.overlay else {
        return;
    };
    let mut changed = false;
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.modals.overlay = None;
        }
        KeyCode::Down | KeyCode::Char('j') => view.step(1),
        KeyCode::Up | KeyCode::Char('k') => view.step(-1),
        KeyCode::PageDown => view.step(i64::from(view.list_area.height.max(1))),
        KeyCode::PageUp => view.step(-i64::from(view.list_area.height.max(1))),
        KeyCode::Home => view.cursor = 0,
        KeyCode::End => view.cursor = view.rows.len().saturating_sub(1),
        KeyCode::Right | KeyCode::Char('l') => changed = view.set_open(true),
        KeyCode::Left | KeyCode::Char('h') => {
            changed = view.set_open(false);
            if !changed {
                if let Some(up) = view.parent_of_cursor() {
                    view.cursor = up;
                }
            }
        }
        KeyCode::Char(' ') => {
            let open = view.rows.get(view.cursor).is_some_and(|r| !r.expanded);
            changed = view.set_open(open);
        }
        _ => {}
    }
    if changed {
        refresh_subagents(app);
    }
    app.chrome.dirty = true;
}

/// A click on a row puts the cursor on it; on the cursor's row it opens
/// the session — or, on the row's fold mark, opens or folds it. The wheel
/// walks the rows.
pub(crate) fn handle_subagents_mouse(
    app: &mut App,
    mouse: MouseEvent,
    out: &mut Vec<ClientRequest>,
) -> bool {
    let Some(Overlay::Subagents(view)) = &mut app.modals.overlay else {
        return false;
    };
    match mouse.kind {
        MouseEventKind::ScrollDown => view.step(1),
        MouseEventKind::ScrollUp => view.step(-1),
        MouseEventKind::Down(MouseButton::Left) => {
            let area = view.list_area;
            let inside = mouse.column >= area.x
                && mouse.column < area.right()
                && mouse.row >= area.y
                && mouse.row < area.bottom();
            let Some(row) = inside
                .then(|| view.scroll + usize::from(mouse.row - area.y))
                .filter(|&row| row < view.rows.len())
            else {
                return true;
            };
            let r = &view.rows[row];
            let caret = crate::ui::subagents_view::caret_x(area, r.node.depth);
            if r.has_kids() && mouse.column >= caret && mouse.column < caret + 2 {
                view.cursor = row;
                let open = !r.expanded;
                view.set_open(open);
                refresh_subagents(app);
            } else if row == view.cursor {
                open_row(app, out);
            } else {
                view.cursor = row;
            }
        }
        _ => {}
    }
    app.chrome.dirty = true;
    true
}

// ---- attention ----

/// A press of an ATTENTION STRIP count (or its palette command): the next
/// session in that state, round and round, landed on as `/` Enter lands.
pub(crate) fn jump_attention_kind(app: &mut App, tone: Tone, out: &mut Vec<ClientRequest>) {
    let list = crate::attention::tally(app)
        .remove(&tone)
        .unwrap_or_default();
    let Some(id) = app.desk.attention.next(tone, &list) else {
        app.chrome.flash = Some(format!(
            "no session {}",
            match tone {
                Tone::Needs => "needs you",
                other => other.word(),
            }
        ));
        return;
    };
    let target = PaletteTarget::Session(id);
    let attaches = crate::config::Config::load().palette_enter_attaches;
    let landing = Landing::for_enter_on(app, &target, attaches);
    jump_to_target(app, target, landing, out);
}

// ---- command palette ----

/// `^K`: the `/` palette with every action in it, the selection's verbs
/// on top.
pub(crate) fn open_command_palette(app: &mut App) {
    app.modals.overlay = Some(Overlay::Palette(
        Palette::new(
            &app.tree,
            crate::config::Config::load().palette_enter_attaches,
            &app.github.open_prs,
            app.launcher.hide_draft_prs,
        )
        .with_commands()
        // What can be done to the thing under the cursor, first.
        .with_verbs(super::verbs::palette_verbs(app)),
    ));
    app.chrome.dirty = true;
}

// ---- back / forward ----

/// Where the grid is: the project in front of you, the session card
/// under the cursor, full-screen or not. None before the first project.
pub(crate) fn current_place(app: &App) -> Option<Place> {
    if !app.launcher_active() {
        return None;
    }
    let project = app.selected_project().map(|p| p.id.clone());
    let session = match app.selected_session_row() {
        Some(SessionRow::Agent(a)) if !a.archived => Some(a.id),
        _ => None,
    };
    Some(Place {
        project,
        session,
        full_screen: app.pane.collapsed,
    })
}

/// After a key or a click: note the place the grid is on, for back. Not
/// with an overlay up — the place is the one under it, noted when it goes.
pub(crate) fn note_place(app: &mut App) {
    if app.modals.overlay.is_some() {
        return;
    }
    if let Some(place) = current_place(app) {
        app.desk.history.note(place);
    }
    // A project added since the snapshot gets its Azure DevOps probe.
    super::ado::probe(app);
}

/// `⌥←` (`delta` -1) / `⌥→` (1), or the header's `‹ ›`: the place before
/// or after this one, landed on as a `/` pick lands, then full-screen or
/// not as it was.
pub(crate) fn go(app: &mut App, delta: i64, out: &mut Vec<ClientRequest>) {
    // The place now first, so a back from a place no key noted yet
    // comes forward to it again.
    note_place(app);
    let Some(place) = app.desk.history.step(delta) else {
        app.chrome.flash = Some(if delta < 0 {
            "nothing to go back to".into()
        } else {
            "nothing to go forward to".into()
        });
        return;
    };
    if app.pane.collapsed && !place.full_screen {
        launcher::toggle_full_screen(app, out);
    }
    match (&place.session, &place.project) {
        (Some(id), _) => {
            jump_to_target(
                app,
                PaletteTarget::Session(id.clone()),
                Landing::FocusOnly,
                out,
            );
        }
        (None, Some(project)) => {
            jump_to_target(
                app,
                PaletteTarget::Project(project.clone()),
                Landing::FocusOnly,
                out,
            );
        }
        (None, None) => {}
    }
    if place.full_screen && !app.pane.collapsed {
        launcher::toggle_full_screen(app, out);
    }
    note_place(app);
    app.chrome.dirty = true;
}

/// The actions this module routes, from `key_modes`.
pub(crate) fn handle_action(
    app: &mut App,
    action: crate::keymap::Action,
    out: &mut Vec<ClientRequest>,
) {
    use crate::keymap::Action;
    match action {
        Action::CommandPalette => open_command_palette(app),
        Action::NavBack => go(app, -1, out),
        Action::NavForward => go(app, 1, out),
        Action::Subagents => open_subagents(app, None),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{buffer_text, hse, press, seed_tree, with_default_config};
    use super::*;
    use nebula_core::{AgentStatus, Entity, SubagentStatus};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn add_agent(app: &mut App, id: &str, status: AgentStatus) {
        let mut a = app.tree.agents[0].clone();
        a.id = AgentId(id.into());
        a.name = id.into();
        a.status = status;
        a.sort_order = app.tree.agents.len() as i64;
        hse(
            app,
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            },
        );
    }

    fn sub(parent: &str, id: &str, status: SubagentStatus) -> SubagentSummary {
        SubagentSummary {
            id: id.into(),
            parent_agent: AgentId(parent.into()),
            kind: "Explore".into(),
            description: format!("look into {id}"),
            started_at: 1,
            ended_at: (status != SubagentStatus::Running).then_some(2),
            status,
            run: None,
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.launcher.launcher_pane_at = crate::launcher::PaneSide::Bottom;
        seed_tree(&mut app);
        app
    }

    fn draw(app: &mut App) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(160, 34)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, app)).unwrap();
        terminal
    }

    fn cell(app: &App, hit: &HitTarget) -> (u16, u16) {
        let (rect, _) = app
            .chrome
            .hits
            .iter()
            .find(|(_, h)| h == hit)
            .unwrap_or_else(|| panic!("{hit:?} was not drawn"));
        (rect.x, rect.y)
    }

    fn click(app: &mut App, (column, row): (u16, u16)) {
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        handle_terminal_event(app, crossterm::event::Event::Mouse(event), &mut Vec::new());
    }

    fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
        handle_terminal_event(
            app,
            crossterm::event::Event::Key(KeyEvent::new(code, mods)),
            &mut Vec::new(),
        );
    }

    fn tree_keys(app: &App) -> Vec<String> {
        let Some(Overlay::Subagents(view)) = &app.modals.overlay else {
            panic!("no subagents overlay: {:?}", app.modals.overlay);
        };
        view.rows.iter().map(|r| r.node.key.clone()).collect()
    }

    /// A subagent the daemon reports puts `⑂ running/all` on its session's
    /// card; a click on it opens the SUBAGENTS OVERLAY on that session,
    /// its subagent unfolded under it.
    #[test]
    fn a_subagent_puts_a_chip_on_its_card_that_opens_the_tree() {
        with_default_config(|| {
            let mut app = app();
            hse(
                &mut app,
                ServerEvent::SubagentChanged {
                    sub: sub("a1", "x", SubagentStatus::Running),
                },
            );
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("⑂ 1/1"), "{text}");
            // The selected card's buttons share the border with it, from
            // the other end.
            assert!(
                app.chrome
                    .hits
                    .iter()
                    .any(|(_, h)| matches!(h, HitTarget::CardButton(..))),
                "{text}"
            );
            let at = cell(&app, &HitTarget::SubagentChip(AgentId("a1".into())));
            click(&mut app, at);
            assert_eq!(tree_keys(&app), vec!["a:a1", "s:a1:x"]);
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("Sessions · demo"), "{text}");
            assert!(text.contains("look into x"), "{text}");

            // Ended, the chip stays with nothing running.
            hse(
                &mut app,
                ServerEvent::SubagentChanged {
                    sub: sub("a1", "x", SubagentStatus::Done),
                },
            );
            app.modals.overlay = None;
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("⑂ 0/1"), "{text}");
        });
    }

    /// The card lists its running subagent under the prompt; once it ends
    /// it folds into the `1 finished` count, the tree shows it under that
    /// fold, `x` asks the daemon to clear it, and the daemon's removal
    /// takes it off the card.
    /// A card's subagent row names the model and effort it runs at, dim
    /// after its task, when the daemon knows them.
    #[test]
    fn a_card_subagent_row_names_its_model_and_effort() {
        with_default_config(|| {
            let mut app = app();
            let mut x = sub("a1", "x", SubagentStatus::Running);
            x.run = nebula_core::SubagentRun::known("claude-haiku-4-5".into(), "low".into());
            hse(&mut app, ServerEvent::SubagentChanged { sub: x });
            let line = |width: usize| -> String {
                let th = app.chrome.theme;
                let lines = crate::ui::subagents_view::card_lines(
                    &app,
                    &AgentId("a1".into()),
                    width,
                    false,
                    th,
                );
                lines[0].iter().map(|s| s.content.as_ref()).collect()
            };
            assert!(
                line(60).starts_with("⑂ look into x  haiku-4-5 · low"),
                "{}",
                line(60)
            );
            // A narrow card keeps the name and drops them.
            assert!(!line(34).contains("haiku"), "{}", line(34));
        });
    }

    #[test]
    fn a_card_lists_its_subagents_and_x_clears_the_finished_ones() {
        with_default_config(|| {
            let mut app = app();
            let mut out = Vec::new();
            hse(
                &mut app,
                ServerEvent::SubagentChanged {
                    sub: sub("a1", "x", SubagentStatus::Running),
                },
            );
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("⑂ look into x"), "{text}");
            assert!(text.contains("running"), "{text}");

            hse(
                &mut app,
                ServerEvent::SubagentChanged {
                    sub: sub("a1", "x", SubagentStatus::Done),
                },
            );
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("⑂ 1 finished"), "{text}");
            assert!(!text.contains("⑂ look into x"), "{text}");

            press(&mut app, KeyCode::Char('S'), KeyModifiers::SHIFT, &mut out);
            assert_eq!(tree_keys(&app), vec!["a:a1", "f:a1"], "folded by default");
            press(&mut app, KeyCode::Down, KeyModifiers::NONE, &mut out);
            press(&mut app, KeyCode::Right, KeyModifiers::NONE, &mut out);
            assert_eq!(tree_keys(&app), vec!["a:a1", "f:a1", "s:a1:x"]);

            out.clear();
            press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE, &mut out);
            assert!(
                out.iter().any(|r| matches!(
                    r,
                    ClientRequest::ClearEndedSubagents { parent: Some(p), .. } if p.0 == "a1"
                )),
                "{out:?}"
            );
            hse(
                &mut app,
                ServerEvent::SubagentsRemoved {
                    parent: AgentId("a1".into()),
                    ids: vec!["x".into()],
                },
            );
            assert_eq!(tree_keys(&app), vec!["a:a1"]);
            app.modals.overlay = None;
            let text = buffer_text(&draw(&mut app));
            assert!(!text.contains("⑂ 1 finished"), "{text}");
        });
    }

    /// With `hide_done_subagents` off the ended ones show one by one,
    /// with their own words.
    #[test]
    fn shown_one_by_one_an_ended_subagent_says_how_it_ended() {
        with_default_config(|| {
            let mut app = app();
            app.desk.subagents.hide_done = false;
            hse(
                &mut app,
                ServerEvent::SubagentChanged {
                    sub: sub("a1", "x", SubagentStatus::EndedUnknown),
                },
            );
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("⑂ look into x"), "{text}");
            assert!(text.contains("closed"), "{text}");
        });
    }

    /// `⇧S` opens the tree on the card under the cursor; a snapshot
    /// replaces the book under it; Enter on a subagent opens the session
    /// it runs inside.
    #[test]
    fn the_tree_follows_the_snapshot_and_enter_opens_the_parent() {
        with_default_config(|| {
            let mut app = app();
            add_agent(&mut app, "a2", AgentStatus::Running);
            on_snapshot_subagents(&mut app, vec![sub("a2", "y", SubagentStatus::Running)]);
            let mut out = Vec::new();
            press(&mut app, KeyCode::Char('S'), KeyModifiers::SHIFT, &mut out);
            assert_eq!(tree_keys(&app), vec!["a:a1", "a:a2", "s:a2:y"]);
            press(&mut app, KeyCode::End, KeyModifiers::NONE, &mut out);
            press(&mut app, KeyCode::Left, KeyModifiers::NONE, &mut out);
            if let Some(Overlay::Subagents(v)) = &app.modals.overlay {
                assert_eq!(v.cursor, 1, "← on a leaf steps to its session");
            }
            press(&mut app, KeyCode::Left, KeyModifiers::NONE, &mut out);
            assert_eq!(tree_keys(&app), vec!["a:a1", "a:a2"], "← folds it");
            press(&mut app, KeyCode::Right, KeyModifiers::NONE, &mut out);
            press(&mut app, KeyCode::Down, KeyModifiers::NONE, &mut out);
            press(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut out);
            assert!(app.modals.overlay.is_none());
            assert!(matches!(
                app.selected_session_row(),
                Some(SessionRow::Agent(a)) if a.id == AgentId("a2".into())
            ));
        });
    }

    /// `^K` is the `/` palette with every action in it: typed, an action
    /// runs as its key would.
    #[test]
    fn ctrl_k_finds_an_action_by_name_and_runs_it() {
        with_default_config(|| {
            let mut app = app();
            let mut out = Vec::new();
            press(
                &mut app,
                KeyCode::Char('k'),
                KeyModifiers::CONTROL,
                &mut out,
            );
            let Some(Overlay::Palette(p)) = &app.modals.overlay else {
                panic!("no palette");
            };
            assert!(p.commands);
            for c in "accounts and usage".chars() {
                press(&mut app, KeyCode::Char(c), KeyModifiers::NONE, &mut out);
            }
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("Accounts and usage"), "{text}");
            press(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut out);
            assert!(
                matches!(app.modals.overlay, Some(Overlay::Accounts(_))),
                "{:?}",
                app.modals.overlay
            );
            // `/` stays the jump palette alone.
            app.modals.overlay = None;
            press(&mut app, KeyCode::Char('/'), KeyModifiers::NONE, &mut out);
            let Some(Overlay::Palette(p)) = &app.modals.overlay else {
                panic!("no palette");
            };
            assert!(!p.commands);
            assert!(!p
                .list
                .items
                .iter()
                .any(|i| matches!(i.target, PaletteTarget::Action(_))));
        });
    }

    /// The header counts what needs you, failed, runs and is unread across
    /// every project — a running subagent counting as running — and a
    /// click on a count lands on the next session in that state.
    #[test]
    fn the_attention_strip_counts_and_a_click_opens_the_next() {
        with_default_config(|| {
            let mut app = app();
            add_agent(&mut app, "ask", AgentStatus::NeedsFeedback);
            on_snapshot_subagents(&mut app, vec![sub("a1", "z", SubagentStatus::Running)]);
            let text = buffer_text(&draw(&mut app));
            assert!(
                text.contains("1 needs you · 1 running · 0 unread"),
                "{text}"
            );
            assert!(!text.contains("failed"), "{text}");
            let at = cell(&app, &HitTarget::Attention(Tone::Needs));
            click(&mut app, at);
            assert!(matches!(
                app.selected_session_row(),
                Some(SessionRow::Agent(a)) if a.id == AgentId("ask".into())
            ));
        });
    }

    /// The header's `commands` button opens what `^K` opens, and says so.
    #[test]
    fn the_commands_button_is_the_command_palette() {
        with_default_config(|| {
            let mut app = app();
            let text = buffer_text(&draw(&mut app));
            assert!(text.contains("commands ^k"), "{text}");
            let at = cell(&app, &HitTarget::CommandsButton);
            click(&mut app, at);
            assert!(matches!(
                &app.modals.overlay,
                Some(Overlay::Palette(p)) if p.commands
            ));
        });
    }

    /// `⌥←` goes back to the card the grid was on before, `⌥→` forward
    /// again; the header's `‹` does what `⌥←` does.
    #[test]
    fn alt_left_and_right_walk_the_places() {
        with_default_config(|| {
            let mut app = app();
            add_agent(&mut app, "a2", AgentStatus::Running);
            let on = |app: &App| match app.selected_session_row() {
                Some(SessionRow::Agent(a)) => a.id.0,
                _ => String::new(),
            };
            draw(&mut app);
            jump_to_target(
                &mut app,
                PaletteTarget::Session(AgentId("a1".into())),
                Landing::FocusOnly,
                &mut Vec::new(),
            );
            note_place(&mut app);
            jump_to_target(
                &mut app,
                PaletteTarget::Session(AgentId("a2".into())),
                Landing::FocusOnly,
                &mut Vec::new(),
            );
            note_place(&mut app);
            assert_eq!(on(&app), "a2");
            key(&mut app, KeyCode::Left, KeyModifiers::ALT);
            assert_eq!(on(&app), "a1");
            key(&mut app, KeyCode::Right, KeyModifiers::ALT);
            assert_eq!(on(&app), "a2");
            draw(&mut app);
            let at = cell(&app, &HitTarget::NavBack);
            click(&mut app, at);
            assert_eq!(on(&app), "a1");
        });
    }
}
