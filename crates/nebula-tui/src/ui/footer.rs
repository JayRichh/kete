use super::*;

struct FooterHint {
    hints: Span<'static>,
    key_bar: Option<Vec<KeyHint>>,
    /// Between a KEY BAR entry's key and its words: the colon the
    /// footer's prose has always used, or the NESTED layout's bare space.
    sep: &'static str,
}

/// One entry of the footer's KEY BAR: the key, what it does, and the
/// action a click on it runs — None for the entries that are a walk of
/// several keys (`hjkl: move`) rather than one button.
struct KeyHint {
    key: String,
    does: String,
    action: Option<Action>,
}

/// A clickable [`KeyHint`]: `key` names `action` and runs it.
fn button(key: String, does: impl Into<String>, action: Action) -> KeyHint {
    KeyHint {
        key,
        does: does.into(),
        action: Some(action),
    }
}

/// A [`KeyHint`] that only says what keys do — no button.
fn keys(key: String, does: impl Into<String>) -> KeyHint {
    KeyHint {
        key,
        does: does.into(),
        action: None,
    }
}

/// The most a KEY BAR shows: `? more` and six. A footer that lists every
/// key is one nobody reads; the `?` menu and F1 hold the rest.
pub(crate) const FOOTER_MAX: usize = 7;

fn key_bar(hints: Vec<KeyHint>) -> FooterHint {
    FooterHint {
        hints: Span::raw(""),
        key_bar: Some(hints),
        sep: ": ",
    }
}

pub(super) fn draw_footer_bar(f: &mut Frame, app: &mut App, area: Rect) {
    // `area` includes the blank padding row; the bar itself is its last row.
    let area = Rect {
        y: area.y + area.height.saturating_sub(1),
        height: area.height.min(1),
        ..area
    };
    let th = app.chrome.theme;
    let FooterHint {
        hints,
        key_bar,
        sep,
    } = footer_hint(app);
    // On the grid, a KEY BAR that would run past the bar loses the
    // breadcrumb first: the header's lit tab and the pane's header already
    // say the project, the checkout and the card, and the buttons are
    // what the footer is for there.
    let room = usize::from(area.width).saturating_sub(footer_usage_width(app));
    let bar_w: usize = key_bar.as_ref().map_or(0, |bar| {
        bar.iter()
            .map(|h| h.key.chars().count() + sep.chars().count() + h.does.chars().count() + 2)
            .sum()
    });
    let mut context = footer_left_context(app, th, true);
    let context_w: usize = context.iter().map(|s| s.width()).sum();
    if key_bar.is_some() && app.launcher_grid() && context_w + PLATE_ROOM + bar_w > room {
        context = footer_left_context(app, th, false);
    }
    let mut spans: FooterSpans = context.into_iter().map(|s| (s, None)).collect();
    let actions = append_footer_hint(app, &mut spans, hints, key_bar, sep, th);
    render_footer_left(f, app, area, spans, &actions, th);
    render_footer_usage(f, app, area, th);
}

fn footer_hint(app: &App) -> FooterHint {
    let th = app.chrome.theme;
    // A long paste the host typed out takes a moment to read: say so, or
    // the screen looks hung (PASTE BURSTS).
    if let Some(bytes) = app.chrome.pasting {
        return span_hint(Span::styled(
            pasting_label(bytes),
            Style::default().fg(th.warn),
        ));
    }
    if let Some(flash) = &app.chrome.flash {
        return span_hint(Span::styled(flash.clone(), Style::default().fg(th.warn)));
    }
    if app.pane.vim.is_some() {
        return dim_hint(":wq / :q to finish  Ctrl+Q: force close", th);
    }
    if let Some(overlay) = &app.modals.overlay {
        return overlay_footer_hint(app, overlay, th);
    }
    if let Some(tip) = button_tooltip(app, th) {
        return tip;
    }
    if app.splash_showing() {
        return splash_footer_hint(app, th);
    }
    if app.launcher_grid()
        && app.nav.focus != Focus::Terminal
        && app.launcher.launcher_tab_cursor.is_some()
    {
        return launcher_tabs_footer_hint(app, th);
    }
    if app.launcher_grid() && app.nav.focus != Focus::Terminal {
        return launcher_grid_footer_hint(app, th);
    }
    pane_footer_hint(app, th)
}

/// `pasting… 64 KB`, counted in whole KB.
pub(crate) fn pasting_label(bytes: usize) -> String {
    format!("pasting… {} KB", bytes.div_ceil(1024))
}

fn span_hint(hints: Span<'static>) -> FooterHint {
    FooterHint {
        hints,
        key_bar: None,
        sep: ": ",
    }
}

fn dim_hint(text: impl Into<std::borrow::Cow<'static, str>>, th: Theme) -> FooterHint {
    span_hint(Span::styled(text, Style::default().fg(th.dim)))
}

fn overlay_footer_hint(app: &App, overlay: &Overlay, th: Theme) -> FooterHint {
    match overlay {
        Overlay::Grep(view) => dim_hint(
            format!(
                "type: search  ↑/↓: move  Enter: edit in {}  Ctrl+u: clear  Esc: clear/close",
                editor_name(&view.editor)
            ),
            th,
        ),
        Overlay::Diff(view) if view.attribution.session.as_ref().is_some_and(|s| s.offered()) => dim_hint(
            if view.tree.is_some() {
                "type: filter  ↑/↓: move  ←/→: fold  ⇧↑/↓: scroll  Ctrl+n/p: change  Ctrl+s: layout  Ctrl+o: context  Ctrl+y: stage  Ctrl+x: discard  Ctrl+g: vs base/HEAD  Ctrl+l: this session/all  Ctrl+t: flat list  Esc: clear/close"
            } else {
                "type: filter  ↑/↓: file  ⇧↑/↓: scroll  Ctrl+n/p: change  Ctrl+s: layout  Ctrl+o: context  Ctrl+y: stage  Ctrl+x: discard  Ctrl+g: vs base/HEAD  Ctrl+l: this session/all  Ctrl+t: tree  Esc: clear/close"
            },
            th,
        ),
        Overlay::Diff(view) => dim_hint(
            if view.tree.is_some() {
                "type: filter  ↑/↓: move  ←/→: fold  ⇧↑/↓: scroll  Ctrl+n/p: change  Ctrl+s: layout  Ctrl+o: context  Ctrl+y: stage  Ctrl+x: discard  Ctrl+g: vs base/HEAD  Ctrl+t: flat list  Esc: clear/close"
            } else {
                "type: filter  ↑/↓: file  ⇧↑/↓: scroll  Ctrl+n/p: change  Ctrl+s: layout  Ctrl+o: context  Ctrl+y: stage  Ctrl+x: discard  Ctrl+g: vs base/HEAD  Ctrl+t: tree  Esc: clear/close"
            },
            th,
        ),
        Overlay::FileTabs(view) => dim_hint(
            file_tabs_keys_hint(view, app.pane.vim.as_ref().is_some_and(|v| v.embedded)),
            th,
        ),
        Overlay::Tree(view) => {
            let md = markdown_toggle_hint("Ctrl+r", view.markdown, view.pretty);
            dim_hint(
                format!(
                    "type: filter  ↑/↓: move  ←/→: fold  Enter: open/edit  ⇧↑/↓: scroll{md}  \
                     Ctrl+u: clear filter  Esc: clear/close"
                ),
                th,
            )
        }
        Overlay::Files(view) => {
            // A markdown selection is read first (the FILE TABS); the hint
            // says so rather than promising the editor.
            let enter = if view
                .selected_path()
                .is_some_and(crate::markdown::is_markdown_path)
            {
                "Enter: preview".to_string()
            } else {
                format!("Enter: edit in {}", editor_name(&view.editor))
            };
            dim_hint(
                format!(
                    "type: search  ↑/↓: move  {enter}  Ctrl+y: copy path  Ctrl+u: clear  Esc: clear/close"
                ),
                th,
            )
        }
        Overlay::Ado(_) => dim_hint("↑/↓: move  Tab/1-3: list  Enter: open  Esc: close", th),
        Overlay::Palette(p) if p.commands => dim_hint(
            "type: search  ↑/↓: move  Enter: jump or run  Ctrl+u: clear  Esc: clear/close",
            th,
        ),
        Overlay::Subagents(_) => dim_hint(
            "↑/↓: move  →/←: open/fold  Enter: open session  d: fold/show finished  x: clear finished (X: all)  Tab: project/all  Esc: close",
            th,
        ),
        Overlay::Palette(_) => dim_hint(
            "type: search  ↑/↓: move  Enter: open  Ctrl+u: clear  Esc: clear/close",
            th,
        ),
        Overlay::Settings(view) => dim_hint(settings_keys_hint(view), th),
        Overlay::Metrics(_) => dim_hint(
            "↑/↓: select  Enter: open session  Esc: close  (refreshes every 2s)",
            th,
        ),
        Overlay::Hosts(view) => dim_hint(
            if view.input.is_some() {
                "type user@host [dir]  Enter: connect (restarts kete over ssh)  Esc: cancel"
            } else {
                "↑/↓: select  Enter: connect (restarts kete over ssh)  a: new  d: remove  Esc: close"
            },
            th,
        ),
        Overlay::AgentPresets(view) => dim_hint(crate::preset_overlays::footer_hint(view), th),
        Overlay::AgentPresetEditor(_) => dim_hint(
            "Tab/↑↓: next field  ←/→: cycle  Shift+Enter/^J: newline  Enter: save  Esc: back to list",
            th,
        ),
        Overlay::Issues(view) => dim_hint(crate::issues::footer_hint(view), th),
        Overlay::PullRequests(_) => dim_hint(crate::pr_modal::footer_hint(), th),
        Overlay::BranchSwitch(view) => dim_hint(crate::branch_switch::footer_hint(view), th),
        Overlay::Menu(m) if m.is_project_picker() => dim_hint(
            "type: filter  Enter: open the project  ↑/↓: move  Esc: close",
            th,
        ),
        Overlay::Menu(m) => menu_footer_hint(m)
            .map(|hint| dim_hint(hint, th))
            .unwrap_or_else(|| dim_hint("Esc: close  Enter: confirm", th)),
        Overlay::Prompt(p) if matches!(p.kind, crate::app::PromptKind::QuickPrompt(_)) => {
            quick_prompt_footer_hint(app, th)
        }
        Overlay::Accounts(view) => dim_hint(
            match view.mode {
                crate::accounts_overlay::Mode::List
                    if view.tab == crate::accounts_overlay::Tab::Wake =>
                {
                    crate::wake_tab::FOOTER_HINT
                }
                crate::accounts_overlay::Mode::List => {
                    "↑/↓: move  a: add  Enter: edit  Space: on/off  d: default  r: rename  l: sign in  x: remove  w: wake now  Tab: wake-up  Esc: close"
                }
                crate::accounts_overlay::Mode::Form(_) => {
                    "Tab/↑/↓: field  ←/→/Space: change  type: fill  Del: clear variables  Enter: save  Esc: back"
                }
                crate::accounts_overlay::Mode::Remove { home: None, .. } => "y/Enter: remove  n/Esc: keep it",
                crate::accounts_overlay::Mode::Remove { .. } => {
                    "y/Enter: remove, keep its home  p: remove and delete its home  n/Esc: keep it"
                }
                crate::accounts_overlay::Mode::UseFor { .. } => {
                    "↑/↓: move  Enter: set / clear the repo's default  Esc: back"
                }
            },
            th,
        ),
        Overlay::ProjectPicker(_) => dim_hint(
            "type: filter projects  ↑/↓: move  Enter: aim the box there  Esc: clear/back to the box",
            th,
        ),
        _ => dim_hint("Esc: close  Enter: confirm", th),
    }
}

fn quick_prompt_footer_hint(app: &App, th: Theme) -> FooterHint {
    // `^P`, `^O`, `Tab` and `^N` are on the box itself now, each
    // beside the thing it changes — a third copy down here was most
    // of what made this screen read as a wall of chords. A box
    // standing on a modal goes back to it.
    let hint = match app
        .modals
        .overlay
        .as_ref()
        .and_then(crate::quick_prompt::modal_under)
    {
        Some(crate::quick_prompt::ModalUnder::Issues(_)) => {
            "Enter: launch  ⇧Tab: preset  Esc: back to issues"
        }
        Some(crate::quick_prompt::ModalUnder::PullRequests(_)) => {
            "Enter: launch  ⇧Tab: preset  Esc: back to pull requests"
        }
        None => "Enter: launch  ⇧Tab: preset  Esc: back to sessions",
    };
    dim_hint(hint, th)
}

/// What the footer says while the pointer rests on a button that is not
/// in it — a card's, a band's `+`, the header's and the pane header's:
/// the button's key and what it does, the Hotkeys tab's own words. The
/// TUI's tooltip, so no button is a mystery until it is clicked.
fn button_tooltip(app: &App, th: Theme) -> Option<FooterHint> {
    let action = match app.launcher.hover_crumb.as_ref()? {
        HitTarget::Button(action)
        | HitTarget::CardButton(_, action)
        | HitTarget::KanbanButton(_, action) => *action,
        HitTarget::LauncherBandNew(_) => Action::QuickPrompt,
        HitTarget::CardMenu(_) | HitTarget::KanbanMenu(_) | HitTarget::LauncherPaneMenu => {
            return Some(dim_hint(
                "click: the card's menu (a right-click on any card opens its own)",
                th,
            ));
        }
        HitTarget::Tip(crate::learn::TipButton::GotIt) => {
            return Some(dim_hint(
                "click: hide this tip, seen now (any key does too once it has been up a moment)",
                th,
            ));
        }
        HitTarget::Tip(crate::learn::TipButton::Off) => {
            return Some(dim_hint(
                format!(
                    "click: no more tips · {}: Settings, General, Show tips turns them back on",
                    key_hint(app, Action::Settings)
                ),
                th,
            ));
        }
        HitTarget::ScopeChip(0) | HitTarget::ScopeClear => {
            return Some(dim_hint(
                format!(
                    "click: the scope back to everything (Esc, once no card is aimed at) · {} on the board flips this project / every project · Focus here (?) narrows it",
                    key_hint(app, Action::FocusNext)
                ),
                th,
            ));
        }
        HitTarget::ScopeChip(_) => {
            return Some(dim_hint(
                "click: widen the scope to this · Focus here (? on a tab, band or card) narrows it",
                th,
            ));
        }
        HitTarget::ScopeAccount => {
            return Some(dim_hint(
                format!(
                    "click: scope to one account's sessions, or any account · Scope to an account… in {} does the same",
                    key_hint(app, Action::CommandPalette)
                ),
                th,
            ));
        }
        HitTarget::AccountChip(_) => {
            return Some(dim_hint(
                format!(
                    "click: move this session to another account (restarts it fresh) · keys: {} then m on an account",
                    key_hint(app, Action::Accounts)
                ),
                th,
            ));
        }
        _ => return None,
    };
    let spec = crate::keymap::spec_of(action)?;
    Some(span_hint(Span::styled(
        format!("{}: {} · {}", key_hint(app, action), spec.label, spec.hint),
        Style::default().fg(th.text),
    )))
}

fn splash_footer_hint(app: &App, _th: Theme) -> FooterHint {
    // The splash covers the panels, so every panel hotkey is dead here.
    // List only what actually fires.
    let k = |a| key_hint(app, a);
    // Launched inside a repo: Enter opens it, and `o` is for any other folder.
    let mut hints = Vec::new();
    if let Some(name) = app.launch_repo_name() {
        hints.push(button(
            k(Action::Activate),
            format!("open {name}"),
            Action::Activate,
        ));
    }
    let folder = if hints.is_empty() {
        "open a folder"
    } else {
        "open another folder"
    };
    hints.extend([
        button(k(Action::AddProject), folder, Action::AddProject),
        button(k(Action::Hosts), "ssh host", Action::Hosts),
        button(k(Action::Settings), "settings", Action::Settings),
        button(k(Action::Help), "help", Action::Help),
        button(k(Action::Quit), "quit", Action::Quit),
    ]);
    key_bar(hints)
}

fn launcher_tabs_footer_hint(app: &App, _th: Theme) -> FooterHint {
    // The LAUNCHER VIEW's PROJECT TABS holding the keys (`k`,`k` off the top
    // row of cards): walking the header's cursor, switching the grid as it
    // goes, and the ways back down.
    let k = |a| key_hint(app, a);
    let down = k(Action::MoveDown);
    key_bar(vec![
        button(k(Action::Verbs), "more", Action::Verbs),
        keys(
            format!("{}{}", k(Action::FocusLeft), k(Action::FocusRight)),
            "switch project",
        ),
        button(
            format!("{} or {down}{down}", k(Action::Activate)),
            "into its cards",
            Action::Activate,
        ),
        button(
            k(Action::CloseProjectTab),
            "close tab",
            Action::CloseProjectTab,
        ),
        keys("esc".into(), "back to the cards"),
        button(k(Action::Quit), "quit", Action::Quit),
    ])
}

fn launcher_grid_footer_hint(app: &App, _th: Theme) -> FooterHint {
    let k = |a| key_hint(app, a);
    let move_keys = format!(
        "{}{}{}{}",
        k(Action::FocusLeft),
        k(Action::MoveDown),
        k(Action::MoveUp),
        k(Action::FocusRight),
    );
    if app.launcher.launcher_nested && !app.launcher.show_archived {
        let walk = format!("{}{}", k(Action::MoveDown), k(Action::MoveUp));
        return FooterHint {
            sep: " ",
            ..key_bar(vec![
                keys(walk, "move"),
                button(k(Action::Activate), "open", Action::Activate),
                button(k(Action::QuickPrompt), "sub-prompt", Action::QuickPrompt),
            ])
        };
    }
    // Every entry that is one action's key is a button: a click on it runs
    // that action, as the key does. The walks (`hjkl`, `[]`) stay words.
    // `?` leads the bar, so the one button every other verb is a click
    // away from — the selection's verbs, the last of them every key — is
    // never the one a narrow bar cuts.
    let more = button(k(Action::Verbs), "more", Action::Verbs);
    // Then how to walk, the selection's own verbs (the registry's leading
    // four for what is under the cursor, in its words), and the view's.
    let selected: Vec<KeyHint> =
        crate::event_loop::verbs::primary(app, nebula_config::actions::PRIMARY_SHOWN, true)
            .into_iter()
            .map(|(action, word)| button(k(action), word, action))
            .collect();
    let mut hints = if app.launcher.launcher_kanban {
        // `Tab` is the scope chip's old switch: a narrowed scope back to
        // everything, else this project / every project.
        let scope = if !app.launcher.scope.is_everything() || !app.launcher.kanban_all {
            "all"
        } else {
            "this project"
        };
        // One walk: `hl` across the lanes, `jk` down one.
        let mut hints = vec![keys(move_keys, "move")];
        hints.extend(selected);
        hints.push(button(k(Action::FocusNext), scope, Action::FocusNext));
        hints.push(button(
            k(Action::ToggleKanban),
            "overview",
            Action::ToggleKanban,
        ));
        hints
    } else if app.launcher.show_archived {
        let mut hints = vec![keys(move_keys, "move")];
        hints.extend(selected);
        hints.push(button(
            k(Action::ToggleArchived),
            "back to live sessions",
            Action::ToggleArchived,
        ));
        hints
    } else {
        let mut hints = vec![keys(move_keys, "move")];
        hints.extend(selected);
        // The view's own, most used first: the cap keeps what fits.
        hints.extend([
            button(k(Action::QuickPrompt), "new session", Action::QuickPrompt),
            button(k(Action::ToggleKanban), "board", Action::ToggleKanban),
            keys(
                format!("{}{}", k(Action::PrevProjectTab), k(Action::NextProjectTab)),
                "project tabs",
            ),
        ]);
        hints
    };
    // A verb already on the bar (`new session` where the selection's own
    // is the box) is said once.
    let mut seen = Vec::new();
    hints.retain(|h| match h.action {
        Some(a) if seen.contains(&a) => false,
        Some(a) => {
            seen.push(a);
            true
        }
        None => true,
    });
    hints.insert(0, more);
    // FEWER THINGS TO READ: `? more`, the walk, the selection's leading
    // verbs and the view's first; the rest (jump, quit, help) are a `?`
    // or F1 away.
    hints.truncate(FOOTER_MAX);
    key_bar(hints)
}

/// The columns layout's footer. The terminal's states are words, no verb;
/// a row of the panels is the registry's, as the grid's footer has it:
/// `?` first, the selection's leading verbs in their one word each (a pull
/// request, issue or link row, which no verb is about, its own few), then
/// the focus's way to add, search and help.
fn pane_footer_hint(app: &App, th: Theme) -> FooterHint {
    let k = |a| key_hint(app, a);
    let word = |a: Action| crate::keymap::spec_of(a).map_or("", |s| s.word);
    let verb = |a: Action| button(k(a), word(a), a);
    match app.nav.focus {
        Focus::Terminal if app.previewed_cloud().is_some() => {
            return dim_hint(
                format!(
                    "{}: open in browser  {}: sessions",
                    k(Action::Activate),
                    k(Action::FocusLeft)
                ),
                th,
            )
        }
        Focus::Terminal if app.pane.term.as_ref().is_some_and(|t| t.exited) => {
            return dim_hint("session exited: Esc: back to sessions", th)
        }
        Focus::Terminal if app.pane.term_locked => {
            return dim_hint(locked_terminal_footer_hint(app), th)
        }
        Focus::Terminal if app.pane.term.is_some() => {
            return dim_hint(
                format!(
                    "{}: type into terminal  {}: sessions",
                    k(Action::Activate),
                    k(Action::FocusLeft)
                ),
                th,
            )
        }
        Focus::Terminal => return dim_hint("select a session and press Enter to attach", th),
        _ => {}
    }
    // The rows no registry verb is about: what their keys do there.
    let browse = |a: Action| button(k(a), "open in browser", a);
    let row_own: Option<Vec<KeyHint>> = match app.nav.focus {
        Focus::Worktrees if app.selected_worktree_pr().is_some() => Some(vec![
            browse(Action::Activate),
            verb(Action::GitDiff),
            verb(Action::RefreshPullRequests),
        ]),
        Focus::Worktrees if app.selected_worktree_issue().is_some() => {
            Some(vec![browse(Action::Activate), verb(Action::QuickPrompt)])
        }
        Focus::Sessions if app.selected_link().is_some_and(|row| row.id().is_none()) => Some(vec![
            browse(Action::Activate),
            verb(Action::GitDiff),
            verb(Action::RefreshPullRequests),
        ]),
        Focus::Sessions if app.selected_link().is_some() => Some(vec![
            browse(Action::Activate),
            button(k(Action::Rename), "edit URL", Action::Rename),
            verb(Action::Delete),
        ]),
        _ => None,
    };
    let mut hints = row_own.unwrap_or_else(|| {
        crate::event_loop::verbs::primary(app, nebula_config::actions::PRIMARY_SHOWN, true)
            .into_iter()
            .map(|(action, word)| button(k(action), word, action))
            .collect()
    });
    // The focus's own way to add something, then search and help.
    match app.nav.focus {
        Focus::Projects => hints.push(verb(Action::AddProject)),
        Focus::Worktrees => hints.push(verb(Action::AgentPresets)),
        _ => hints.push(verb(Action::NewTerminal)),
    }
    hints.push(verb(Action::Palette));
    hints.push(verb(Action::Help));
    let mut seen = Vec::new();
    hints.retain(|h| match h.action {
        Some(a) if seen.contains(&a) => false,
        Some(a) => {
            seen.push(a);
            true
        }
        None => true,
    });
    hints.insert(0, verb(Action::Verbs));
    hints.truncate(FOOTER_MAX);
    key_bar(hints)
}

fn locked_terminal_footer_hint(app: &App) -> String {
    format!(
        "{}: {}  {}  ⌥click: open link",
        if app.launcher_grid() {
            app.chrome
                .keymap
                .chords(Action::ToggleLauncherPane)
                .iter()
                .find(|c| !crate::key_combo::is_text_key(c))
                .map(|c| c.display())
        } else {
            None
        }
        .or_else(|| app
            .chrome
            .keymap
            .first(Action::UnlockTerminal)
            .map(|c| c.display()))
        .unwrap_or_else(|| "^q".into()),
        if app.launcher_grid() {
            "back to the card"
        } else if app.launcher_active() && app.pane.collapsed {
            "normal size"
        } else {
            "sessions"
        },
        if app.child_mouse_mode().0 != vt100::MouseProtocolMode::None {
            "drag: to the app (⇧drag: terminal)"
        } else {
            "drag: select+copy"
        },
    )
}

/// The footer's left: the host, the connection, and — with `crumbs` —
/// the breadcrumb.
fn footer_left_context(app: &App, th: Theme, crumbs: bool) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw(" ")];
    if app.chrome.is_remote {
        spans.push(Span::styled(
            truncate(&app.chrome.hostname, 24),
            Style::default().fg(th.warn).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("  ·  ", Style::default().fg(th.dim)));
    }
    if matches!(app.chrome.conn, ConnState::Disconnected) {
        spans.push(Span::styled("✗ disconnected", Style::default().fg(th.err)));
        spans.push(Span::styled("  ·  ", Style::default().fg(th.dim)));
    }
    let crumbs = if crumbs { breadcrumb(app) } else { Vec::new() };
    if !crumbs.is_empty() {
        spans.extend(crumbs);
        // No changed-file count here: it rides each card's branch, where
        // it reads as that checkout's (`launcher_view::draw_card`).
        spans.push(Span::styled("    ", Style::default()));
    }
    spans
}

/// What the version plate takes (`kete v0.48.0  ·  `), give or take an
/// update notice: the room [`draw_footer_bar`] keeps for it.
const PLATE_ROOM: usize = 20;

/// The columns the usage readout at the bar's right end takes.
fn footer_usage_width(app: &App) -> usize {
    footer_usage(app).map_or(0, |s| s.chars().count() + 2)
}

/// The footer's spans, each tagged with the KEY BAR entry it belongs to,
/// if any — so the draw can lay a button's hit rect over exactly the
/// columns its entry took, wherever the plate before it pushed them.
type FooterSpans = Vec<(Span<'static>, Option<usize>)>;

/// Lay the hint after the context: the KEY BAR's entries, the key bright
/// and its words muted, or the one line of prose. Returns the action each
/// entry runs, by the index its spans are tagged with. An entry under the
/// pointer is lit and underlined, as the header's buttons are.
fn append_footer_hint(
    app: &App,
    spans: &mut FooterSpans,
    hints: Span<'static>,
    key_bar: Option<Vec<KeyHint>>,
    sep: &'static str,
    th: Theme,
) -> Vec<Option<Action>> {
    let Some(entries) = key_bar else {
        let mut hints = hints;
        if hints.style.fg == Some(th.dim) {
            hints.style.fg = Some(th.muted);
        }
        spans.push((hints, None));
        return Vec::new();
    };
    let mut actions = Vec::with_capacity(entries.len());
    for (i, hint) in entries.into_iter().enumerate() {
        if i > 0 {
            spans.push((Span::raw("  "), None));
        }
        let hovered = hint
            .action
            .is_some_and(|a| app.launcher.hover_crumb == Some(HitTarget::FooterHint(a)));
        let (mut key, mut does) = (Style::default().fg(th.text), Style::default().fg(th.muted));
        if hovered {
            key = key.fg(th.accent).add_modifier(Modifier::UNDERLINED);
            does = does.fg(th.text).add_modifier(Modifier::UNDERLINED);
        }
        spans.push((Span::styled(hint.key, key), Some(i)));
        spans.push((Span::styled(format!("{sep}{}", hint.does), does), Some(i)));
        actions.push(hint.action);
    }
    actions
}

/// The footer's left side, and the KEY BAR's buttons laid over it: an
/// entry is a button only where it is drawn whole — one the bar had no
/// room for, or cut at its edge, is not one.
fn render_footer_left(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    mut spans: FooterSpans,
    actions: &[Option<Action>],
    th: Theme,
) {
    let usage = footer_usage(app);
    let right_w = usage
        .as_ref()
        .map(|s| s.chars().count() as u16 + 2)
        .unwrap_or(0)
        .min(area.width);
    let left = Rect {
        width: area.width.saturating_sub(right_w),
        ..area
    };
    prepend_version_plate(app, &mut spans, left.width, th);
    let end = usize::from(left.x) + usize::from(left.width);
    // An entry the bar has no room for goes whole, with the gap before it,
    // rather than cut to a dangling `Enter:`.
    let mut x = usize::from(left.x);
    let mut entry: Option<(usize, usize)> = None;
    for (k, (span, tag)) in spans.iter().enumerate() {
        if let Some(i) = *tag {
            if entry.is_none_or(|(at, _)| at != i) {
                entry = Some((i, k));
            }
        }
        x += span.width();
        if tag.is_some() && x > end {
            if let Some((_, k)) = entry {
                spans.truncate(k);
                while matches!(spans.last(), Some((s, None)) if s.content.trim().is_empty()) {
                    spans.pop();
                }
            }
            break;
        }
    }
    // The line of prose hints (`a: add  Enter: edit`): cut at the last
    // whole hint, two spaces apart, that fits.
    let before: usize = spans.iter().rev().skip(1).map(|(s, _)| s.width()).sum();
    if let Some((last, None)) = spans.last_mut() {
        let room = end.saturating_sub(usize::from(left.x) + before);
        if last.width() > room && last.content.contains("  ") {
            let text: String = last.content.chars().take(room).collect();
            let keep = text
                .rfind("  ")
                .map_or("", |at| &text[..at])
                .trim_end()
                .to_string();
            last.content = keep.into();
        }
    }
    let mut x = usize::from(left.x);
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    for (span, tag) in &spans {
        let w = span.width();
        if let Some(i) = *tag {
            match runs.last_mut() {
                Some((at, _, run_w)) if *at == i => *run_w += w,
                _ => runs.push((i, x, w)),
            }
        }
        x += w;
    }
    for (i, from, w) in runs {
        let Some(Some(action)) = actions.get(i) else {
            continue;
        };
        if from + w > end {
            break;
        }
        app.chrome.hits.push((
            Rect {
                x: from as u16,
                width: w as u16,
                ..left
            },
            HitTarget::FooterHint(*action),
        ));
    }
    let spans: Vec<Span<'static>> = spans.into_iter().map(|(s, _)| s).collect();
    f.render_widget(Paragraph::new(Line::from(spans)), left);
}

fn prepend_version_plate(app: &App, spans: &mut FooterSpans, width: u16, th: Theme) {
    let plate = format!("kete v{}", env!("CARGO_PKG_VERSION"));
    let update = app
        .chrome
        .update_available
        .as_ref()
        .map(|v| format!(" ⇡ v{v}"));
    let plate_w = plate.chars().count()
        + update.as_ref().map_or(0, |u| u.chars().count())
        + "  ·  ".chars().count();
    let body_w: usize = spans.iter().map(|(s, _)| s.width()).sum();
    if app.chrome.flash.is_some() && body_w + plate_w > width as usize {
        return;
    }
    let mut plate_spans = vec![(Span::styled(plate, Style::default().fg(th.dim)), None)];
    if let Some(update) = update {
        plate_spans.push((
            Span::styled(
                update,
                Style::default().fg(th.warn).add_modifier(Modifier::BOLD),
            ),
            None,
        ));
    }
    plate_spans.push((Span::styled("  ·  ", Style::default().fg(th.dim)), None));
    spans.splice(1..1, plate_spans);
}

fn render_footer_usage(f: &mut Frame, app: &mut App, area: Rect, th: Theme) {
    let Some(usage) = footer_usage(app) else {
        return;
    };
    let right_w = (usage.chars().count() as u16 + 2).min(area.width);
    let right = Rect {
        x: area.x + area.width.saturating_sub(right_w),
        width: right_w,
        ..area
    };
    // The readout is a button — a click opens the memory modal, as `⇧M` does.
    let span = Span::styled(usage, Style::default().fg(th.dim));
    let span = if app.launcher.hover_crumb == Some(HitTarget::FooterUsage) {
        span.style(
            Style::default()
                .fg(th.text)
                .add_modifier(Modifier::UNDERLINED),
        )
    } else {
        span
    };
    let width = (span.width() as u16).min(right.width);
    app.chrome.hits.push((
        Rect {
            x: right.x + right.width - width,
            width,
            ..right
        },
        HitTarget::FooterUsage,
    ));
    f.render_widget(
        Paragraph::new(Line::from(span)).alignment(ratatui::layout::Alignment::Right),
        right,
    );
}
