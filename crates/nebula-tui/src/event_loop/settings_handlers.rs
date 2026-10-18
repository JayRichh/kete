//! Extracted event-loop helper section.

use super::*;

pub(crate) fn handle_settings_key(app: &mut App, key: KeyEvent) {
    let Some(view) = settings(app) else {
        return;
    };
    if view.capturing() {
        capture_hotkey(app, key);
        return;
    }
    if view.capture.is_some() {
        // Holding a captured chord that already belongs to someone else.
        if key.code == KeyCode::Enter {
            commit_pending_hotkey(app);
        } else if let Some(view) = settings_mut(app) {
            view.capture = None;
            view.info("kept the existing binding");
        }
        return;
    }

    let (tab, selected, on_tabs) = (view.tab, view.selected, view.on_tabs);
    let last = crate::config::tab_len(tab).saturating_sub(1);
    let tabs = crate::config::tab_count();
    let hotkeys = view.is_hotkeys();
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    let cmd = match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => SettingsCmd::Close,
        KeyCode::BackTab => SettingsCmd::Tab((tab + tabs - 1) % tabs),
        KeyCode::Tab if shift => SettingsCmd::Tab((tab + tabs - 1) % tabs),
        KeyCode::Tab => SettingsCmd::Tab((tab + 1) % tabs),
        KeyCode::Char('[') => SettingsCmd::Tab((tab + tabs - 1) % tabs),
        KeyCode::Char(']') => SettingsCmd::Tab((tab + 1) % tabs),
        // 1-9 jump straight to a tab, the fastest route once you know the
        // strip; out-of-range digits are ignored rather than clamped.
        KeyCode::Char(c @ '1'..='9') => {
            let want = c as usize - '1' as usize;
            if want < tabs {
                SettingsCmd::Tab(want)
            } else {
                return;
            }
        }
        // Shift+R: back to the defaults, behind a confirmation. It isn't
        // about a row, so it works from the strip and the list alike.
        KeyCode::Char('R') => SettingsCmd::ResetAll,
        // Shift+E / Shift+I: the settings to a file, or a file's in.
        KeyCode::Char('E') => SettingsCmd::Export,
        KeyCode::Char('I') => SettingsCmd::Import,
        // Shift+D on the Appearance tab: Kete's own look back.
        KeyCode::Char('D') if crate::config::is_appearance_tab(tab) => SettingsCmd::LookDefaults,
        // Shift+L on a CRITTERS row: the collection log.
        KeyCode::Char('L') if !on_tabs && crate::critters::is_critters_row(tab, selected) => {
            crate::critters::open_log(app);
            return;
        }
        // ---- the tab strip has focus ----
        KeyCode::Left | KeyCode::Char('h') if on_tabs => SettingsCmd::Tab((tab + tabs - 1) % tabs),
        KeyCode::Right | KeyCode::Char('l') if on_tabs => SettingsCmd::Tab((tab + 1) % tabs),
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Enter if on_tabs => SettingsCmd::EnterList,
        KeyCode::Up | KeyCode::Char('k') if on_tabs => return,
        // ---- the list has focus ----
        KeyCode::Char('j') | KeyCode::Down => SettingsCmd::Move((selected + 1).min(last)),
        // ↑ off the top row steps onto the tab strip.
        KeyCode::Char('k') | KeyCode::Up if selected == 0 => SettingsCmd::FocusTabs,
        KeyCode::Char('k') | KeyCode::Up => SettingsCmd::Move(selected - 1),
        KeyCode::Enter | KeyCode::Char(' ') => activate::settings_row_cmd(hotkeys, selected),
        KeyCode::Char('a') | KeyCode::Char('+') if hotkeys => SettingsCmd::Capture { add: true },
        KeyCode::Backspace | KeyCode::Delete if hotkeys => SettingsCmd::ResetHotkey,
        KeyCode::Char('x') if hotkeys => SettingsCmd::ClearHotkey,
        // Nothing to cycle on a hotkey row — say so instead of no-op'ing.
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('l') | KeyCode::Right if hotkeys => {
            SettingsCmd::Nudge
        }
        KeyCode::Char('l') | KeyCode::Right => SettingsCmd::Apply(selected, 1),
        KeyCode::Char('h') | KeyCode::Left => SettingsCmd::Apply(selected, -1),
        _ => return,
    };
    run_settings_cmd(app, cmd);
}

/// Run one SETTINGS OVERLAY command: the one place the overlay's state
/// changes, whoever asked. Keys map to commands in `handle_settings_key`
/// and the mouse in `handle_mouse`; neither touches the view itself, so a
/// click on a tab *is* that tab's key, and a second click on a row *is*
/// Enter on it (`activate::settings_row_cmd`).
pub(crate) fn run_settings_cmd(app: &mut App, cmd: SettingsCmd) {
    let Some(view) = settings(app) else {
        return;
    };
    let (tab, selected) = (view.tab, view.selected);
    match cmd {
        SettingsCmd::Close => close_settings(app),
        SettingsCmd::Tab(next) => {
            app.modals.settings_tab = next;
            let row = app.settings_row(next);
            if let Some(view) = settings_mut(app) {
                view.tab = next;
                view.selected = row;
                view.notice = None;
                view.capture = None;
            }
        }
        SettingsCmd::FocusTabs => {
            app.remember_settings_focus(true);
            if let Some(view) = settings_mut(app) {
                view.on_tabs = true;
                view.notice = None;
            }
        }
        SettingsCmd::EnterList => {
            app.remember_settings_focus(false);
            if let Some(view) = settings_mut(app) {
                view.on_tabs = false;
            }
        }
        SettingsCmd::Move(i) => {
            app.remember_settings_row(tab, i);
            if let Some(view) = settings_mut(app) {
                view.selected = i;
                view.notice = None;
            }
        }
        SettingsCmd::Apply(i, delta) => apply_setting_at(app, tab, i, delta),
        SettingsCmd::Capture { add } => {
            if let Some(view) = settings_mut(app) {
                view.capture = Some(crate::app::HotkeyCapture {
                    action: selected,
                    add,
                    pending: None,
                });
                view.notice = None;
            }
        }
        SettingsCmd::ResetHotkey => {
            if edit_keymap(app, |keymap| keymap.reset(selected)) {
                let label = app.chrome.keymap.display_at(selected);
                if let Some(view) = settings_mut(app) {
                    view.info(format!("reset to the default binding: {label}"));
                }
            }
        }
        SettingsCmd::ClearHotkey => {
            if edit_keymap(app, |keymap| keymap.clear(selected)) {
                if let Some(view) = settings_mut(app) {
                    view.warn("unbound: ⌫ puts the default back");
                }
            }
        }
        SettingsCmd::Nudge => {
            if let Some(view) = settings_mut(app) {
                view.info("Enter: rebind   a: add another key   ⌫: default   x: unbind");
            }
        }
        SettingsCmd::LookDefaults => look_defaults(app),
        SettingsCmd::Export => open_prompt(app, PromptKind::ExportSettings),
        SettingsCmd::Import => open_prompt(app, PromptKind::ImportSettings),
        SettingsCmd::ResetAll => {
            // The confirm replaces the overlay; both of its exits put the
            // settings back on screen (see `reset_settings` and the Esc
            // arm of the Confirm handler).
            app.modals.overlay = Some(Overlay::Confirm(ConfirmDialog {
                title: "Reset settings".into(),
                message: "Every setting goes back to its default: theme, editor, agent \
                          defaults,\ntimeouts, the grid's layout, every project's settings, \
                          and all hotkey\nbindings. Your config.json is rewritten and \
                          config.local.json removed;\nthis can't be undone."
                    .into(),
                action: PendingAction::ResetSettings,
                area: ratatui::layout::Rect::default(),
            }));
        }
    }
}

/// The open settings overlay, for handlers that already know it's up.
pub(crate) fn settings(app: &App) -> Option<&SettingsView> {
    match &app.modals.overlay {
        Some(Overlay::Settings(view)) => Some(view),
        _ => None,
    }
}

/// `settings`, mutably.
pub(crate) fn settings_mut(app: &mut App) -> Option<&mut SettingsView> {
    match &mut app.modals.overlay {
        Some(Overlay::Settings(view)) => Some(view),
        _ => None,
    }
}

pub(crate) enum SettingsCmd {
    Close,
    Tab(usize),
    FocusTabs,
    EnterList,
    Move(usize),
    Apply(usize, i32),
    Capture {
        add: bool,
    },
    ResetHotkey,
    ClearHotkey,
    Nudge,
    ResetAll,
    Export,
    Import,
    /// The Appearance tab's `⇧D`: theme, file icons and black background
    /// back to Kete's own.
    LookDefaults,
}

/// The keystroke that lands while the Hotkeys tab is waiting for one.
/// Esc is the only key that can't be bound — it's the way out of here.
pub(crate) fn capture_hotkey(app: &mut App, key: KeyEvent) {
    // Bare modifier presses aren't chords; keep waiting for a real key.
    if matches!(
        key.code,
        KeyCode::Null | KeyCode::CapsLock | KeyCode::NumLock | KeyCode::ScrollLock
    ) || matches!(key.code, KeyCode::Modifier(_))
    {
        return;
    }
    let Some(view) = settings_mut(app) else {
        return;
    };
    let Some(capture) = view.capture.clone() else {
        return;
    };
    if key.code == KeyCode::Esc {
        view.capture = None;
        view.info("rebind cancelled");
        return;
    }
    let chord = crate::keymap::KeyChord::from_event(&key);
    let conflicts = app.chrome.keymap.conflicts(capture.action, &chord);
    if !conflicts.is_empty() {
        // Warn before stealing: the user gets to see who currently owns
        // the key and decide, instead of finding out when that action
        // stops responding.
        let owners = conflicts
            .iter()
            .filter_map(|i| crate::keymap::spec_at(*i))
            .map(|s| format!("\u{201c}{}\u{201d}", s.label))
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(view) = settings_mut(app) {
            view.warn(format!(
                "{chord} is already {owners}: Enter to move it here, Esc to keep it there"
            ));
            if let Some(c) = &mut view.capture {
                c.pending = Some((chord, conflicts));
            }
        }
        return;
    }
    bind_hotkey(app, capture.action, chord, capture.add);
}

/// Enter on the duplicate warning: take the chord anyway.
pub(crate) fn commit_pending_hotkey(app: &mut App) {
    let Some(view) = settings(app) else {
        return;
    };
    let Some(capture) = view.capture.clone() else {
        return;
    };
    let Some((chord, losers)) = capture.pending else {
        return;
    };
    let stolen_from = losers
        .iter()
        .filter_map(|i| crate::keymap::spec_at(*i))
        .map(|s| s.label)
        .collect::<Vec<_>>()
        .join(", ");
    bind_hotkey(app, capture.action, chord, capture.add);
    if let Some(view) = settings_mut(app) {
        if !stolen_from.is_empty() {
            view.warn(format!(
                "{chord} taken from {stolen_from}, which is now unbound there"
            ));
        }
    }
}

/// Write one binding through to the config, then report how likely the
/// host terminal is to actually deliver it.
pub(crate) fn bind_hotkey(app: &mut App, action: usize, chord: crate::keymap::KeyChord, add: bool) {
    let saved = edit_keymap(app, |keymap| keymap.bind(action, chord, add));
    let Some(view) = settings_mut(app) else {
        return;
    };
    view.capture = None;
    if !saved {
        return;
    }
    match crate::keymap::host_warning(&chord) {
        (crate::keymap::Reach::Fine, _) => view.info(format!("bound to {chord}")),
        (_, Some(why)) => view.warn(format!("bound to {chord}, but {why}")),
        (_, None) => view.info(format!("bound to {chord}")),
    }
}

/// Persist a keymap and adopt it. False means the write failed and nothing
/// changed, so callers skip their success message.
pub(crate) fn save_keymap(app: &mut App, keymap: crate::keymap::Keymap) -> bool {
    let mut cfg = crate::config::Config::load();
    // The desktop window's own overrides share the map: keep them.
    nebula_config::actions::merge_tui_overrides(&mut cfg.keybindings, keymap.overrides());
    if !save_config(app, &cfg) {
        return false;
    }
    app.chrome.keymap = keymap;
    true
}

/// Clone the live keymap, let `edit` change it, and persist the result.
/// False means the write failed and the live keymap is untouched.
pub(crate) fn edit_keymap(app: &mut App, edit: impl FnOnce(&mut crate::keymap::Keymap)) -> bool {
    let mut keymap = app.chrome.keymap.clone();
    edit(&mut keymap);
    save_keymap(app, keymap)
}

/// REMEMBER HARNESS (Settings → Experimental): make a launch's harness —
/// and a model or effort picked for it — the defaults the next NEW
/// SESSION PICKER and QUICK PROMPT start from
/// (`Config::remember_launch`). Nothing is written while the switch is
/// off or the pick already is the default; a failed write flashes.
pub(crate) fn remember_launch(
    app: &mut App,
    kind: AgentKind,
    custom: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
) {
    let mut cfg = crate::config::Config::load();
    if cfg.remember_launch(kind, custom, model, effort) {
        save_config(app, &cfg);
    }
}

/// Write the config file, flashing the failure. False when it didn't land.
pub(crate) fn save_config(app: &mut App, cfg: &crate::config::Config) -> bool {
    match app.chrome.config_watch.own_write(|| cfg.save()) {
        Ok(()) => true,
        Err(err) => {
            app.chrome.flash = Some(format!("couldn't save settings: {err}"));
            false
        }
    }
}

pub(crate) fn rekey_project_config(
    app: &mut App,
    old_path: &std::path::Path,
    new_path: &std::path::Path,
) {
    let mut cfg = crate::config::Config::load();
    if !cfg.rekey_project(old_path, new_path) {
        return;
    }
    if save_config(app, &cfg) {
        apply_config(app, &cfg);
    }
}

/// A PROJECT TAB **Default account** row, stepped `delta` (Enter: one on)
/// through none (the spread picks) and each usable profile of `harness`,
/// written in place under the project's repo path, as the desktop and
/// `kete profile set-project` write it.
fn step_project_account(app: &mut App, harness: &str, delta: i32) {
    let Some(repo) = app.selected_project().map(|p| p.repo_path.clone()) else {
        if let Some(view) = settings_mut(app) {
            view.warn("no project selected, this tab edits the one under the cursor");
        }
        return;
    };
    let cfg = crate::config::Config::load();
    let mut choices: Vec<Option<String>> = vec![None];
    choices.extend(
        nebula_core::profiles::for_harness(&cfg.profiles, harness)
            .iter()
            .map(|p| Some(p.name.trim().to_string())),
    );
    let now = cfg.project(&repo).default_profile(harness);
    let at = choices.iter().position(|c| *c == now).unwrap_or(0) as i32;
    let step = if delta == 0 { 1 } else { delta };
    let n = choices.len() as i32;
    let next = choices[(at + step).rem_euclid(n) as usize].clone();
    let (config, local) = crate::config::Config::layer_paths();
    let repo_key = repo.to_string_lossy().into_owned();
    let written = app.chrome.config_watch.own_write(|| {
        if let Some(name) = next.as_deref() {
            let of = nebula_core::profiles::project_default_harness(&cfg.profiles, name)?;
            if of != harness {
                return Err(format!("{name} is a {of} account, not {harness}"));
            }
        }
        let layer =
            nebula_core::profiles::layer_for(&config, &local, nebula_core::profiles::PROJECTS_KEY)?;
        let _lock = if layer == local {
            Some(nebula_core::settings::lock(&config).map_err(|e| e.to_string())?)
        } else {
            None
        };
        nebula_core::profiles::set_project_default_in_file(
            &layer,
            &repo_key,
            harness,
            next.as_deref(),
        )
    });
    match written {
        Ok(()) => {
            crate::quick_prompt::mirror_accounts(app, &crate::config::Config::load());
            if let Some(view) = settings_mut(app) {
                view.info(match next {
                    Some(name) => format!("{harness} launches here that pick none run on {name}"),
                    None => format!("no {harness} default here: the account spread picks"),
                });
            }
        }
        Err(e) => {
            if let Some(view) = settings_mut(app) {
                view.warn(format!("couldn't save it: {e}"));
            }
        }
    }
    app.chrome.dirty = true;
}

pub(crate) fn apply_setting_at(app: &mut App, tab: usize, index: usize, delta: i32) {
    if tab == crate::config::project_tab() {
        if let Some(harness) = crate::config::Config::load().project_account_row(index) {
            step_project_account(app, &harness, delta);
            return;
        }
    }
    if let Some(spec) = crate::config::setting_at(tab, index) {
        // Enter (and a second click) on the theme row lists every theme to
        // pick from, the installed VS Code ones too; ←/→ still step
        // through the presets.
        if spec.kind == crate::config::SettingKind::Theme && delta == 0 {
            open_theme_picker(app);
            return;
        }
        // A PROJECT TAB row edits the selected project's entry — every
        // one of them typed. With no project to edit — an empty tree —
        // say so rather than open a prompt with nowhere to write.
        let project = if spec.kind.is_project() {
            let Some(path) = app.selected_project().map(|p| p.repo_path.clone()) else {
                if let Some(view) = settings_mut(app) {
                    view.warn("no project selected, this tab edits the one under the cursor");
                }
                return;
            };
            Some(path)
        } else {
            None
        };
        if spec.kind.is_text() {
            // Enter (and a second click) on a typed row opens its prompt in
            // the overlay's place; Enter and Esc there both bring the
            // overlay back (`submit_prompt`, the Esc arm). ←/→ have nothing
            // to step through — say so instead of no-op'ing.
            if delta == 0 {
                open_prompt(
                    app,
                    PromptKind::SettingText {
                        kind: spec.kind,
                        project,
                    },
                );
            } else if let Some(view) = settings_mut(app) {
                view.info("Enter: type a value   (empty puts the default back)");
            }
            return;
        }
    }
    let mut cfg = crate::config::Config::load();
    cfg.cycle(tab, index, delta);
    if nebula_core::harness::usable(&cfg.harness_registry()).is_empty() {
        // Refuse the last harness here, where the user is looking, rather
        // than leave `n` with nothing to offer later. Custom registry
        // entries count: with one offered, the built-ins may all go off.
        if let Some(view) = settings_mut(app) {
            view.warn("keep at least one harness enabled");
        }
        return;
    }
    if !save_config(app, &cfg) {
        return;
    }
    apply_config(app, &cfg);
}

/// The THEME PICKER, over the settings overlay: the presets and the two
/// machine-read names, then every installed VS Code theme as
/// `vscode:<label>` (read now: a theme installed since shows up), the one
/// worn ticked and hovered, letters filtering. Esc, or a click outside it,
/// puts the overlay back.
pub(crate) fn open_theme_picker(app: &mut App) {
    let current = crate::config::Config::load().theme;
    let mut names: Vec<String> = crate::theme::THEMES.iter().map(|t| t.to_string()).collect();
    names.extend(
        crate::vscode_theme::installed_names()
            .into_iter()
            .map(|(name, _)| name),
    );
    let items: Vec<MenuItem> = names
        .into_iter()
        .map(|name| {
            let label = if name.eq_ignore_ascii_case(current.trim()) {
                format!("{name} ✓")
            } else {
                name.clone()
            };
            MenuItem::new(label, MenuAction::PickTheme(name))
        })
        .collect();
    let hover = items
        .iter()
        .position(|i| i.label.ends_with(" ✓"))
        .unwrap_or(0);
    let Some(settings) = app.modals.overlay.take() else {
        return;
    };
    app.modals.verbs_under = Some(Box::new(settings));
    app.modals.overlay = Some(Overlay::Menu(crate::app::ContextMenu {
        title: Some("Color theme".into()),
        items: items.clone(),
        at: None,
        hover,
        area: ratatui::layout::Rect::default(),
        scroll: 0,
        parent: None,
        filter: Some(crate::app::MenuFilter {
            query: String::new(),
            all: items,
        }),
    }));
}

/// A row of the THEME PICKER: the settings back on screen, the theme
/// saved and worn at once.
pub(crate) fn pick_theme(app: &mut App, name: &str) {
    // The settings the picker stood on, put back (by the menu, or here);
    // reopened on the remembered row if they were let go.
    if let Some(under) = app.modals.verbs_under.take() {
        app.modals.overlay = Some(*under);
    }
    if !matches!(app.modals.overlay, Some(Overlay::Settings(_))) {
        reopen_settings(app);
    }
    let mut cfg = crate::config::Config::load();
    if cfg.theme == name {
        return;
    }
    cfg.theme = name.to_string();
    if !save_config(app, &cfg) {
        return;
    }
    apply_config(app, &cfg);
    if let Some(view) = settings_mut(app) {
        view.info(format!("color theme: {name}"));
    }
}

/// `⇧D` on the Appearance tab, or its button: the color theme, the file
/// icons and the black background back to Kete's own (`kete-dark`, auto,
/// on). The other rows on the tab are layout, not look, and stay.
pub(crate) fn look_defaults(app: &mut App) {
    let mut cfg = crate::config::Config::load();
    cfg.reset_look();
    if !save_config(app, &cfg) {
        return;
    }
    apply_config(app, &cfg);
    if let Some(view) = settings_mut(app) {
        view.info(format!(
            "Kete's own look: {}, file icons {}, black background {}",
            cfg.theme,
            cfg.icons_word(),
            if cfg.black_background { "on" } else { "off" }
        ));
    }
}

/// LIGHT/DARK AWARENESS: with `auto` or `vscode` picked, look at the
/// machine again — the OS's app theme, VS Code's settings — and repaint
/// in what it resolves to now, if that moved. Cheap enough for a beat: a
/// registry read and a `stat` when nothing changed. Run on the loop's
/// THEME BEAT and whenever the window takes focus back.
pub(crate) fn recheck_theme(app: &mut App) {
    if let Some(theme) = app.chrome.live_theme.recheck() {
        if theme != app.chrome.theme {
            app.chrome.theme = theme;
            app.chrome.dirty = true;
        }
    }
}

/// LIVE RELOAD: the settings files changed under us — the desktop
/// window's Settings view, or a hand edit — so adopt every value the app
/// mirrors, hotkeys included, as startup does. Run on the loop's CONFIG
/// BEAT. True when it reloaded.
///
/// Deferred while the settings overlay is up, or a typed setting's prompt
/// or the reset confirm stands in for it: the change is left untaken (no poll), so the first
/// beat after the overlay closes applies it. Swapping state under the
/// open overlay — a hotkey capture in progress, the row being typed —
/// would be the surprise; the overlay's own saves load fresh and keep a
/// pending change pending (`ConfigWatch::own_write`), so nothing is lost
/// by waiting.
pub(crate) fn reload_foreign_config(app: &mut App) -> bool {
    let settings_up = match &app.modals.overlay {
        Some(Overlay::Settings(_)) => true,
        Some(Overlay::Prompt(prompt)) => matches!(prompt.kind, PromptKind::SettingText { .. }),
        Some(Overlay::Confirm(confirm)) => matches!(confirm.action, PendingAction::ResetSettings),
        _ => false,
    };
    if settings_up || !app.chrome.config_watch.poll() {
        return false;
    }
    let cfg = crate::config::Config::load();
    apply_config(app, &cfg);
    app.chrome.keymap = cfg.keymap();
    app.chrome.dirty = true;
    true
}

/// Adopt every config value the running app mirrors. Shared by startup,
/// the settings overlay and the config beat's live reload
/// (`reload_foreign_config`) so a new setting can't reach one and miss
/// another — the overlay is a live editor, not a restart-to-apply screen.
pub(crate) fn apply_config(app: &mut App, cfg: &crate::config::Config) {
    crate::learn::sync(app, cfg);
    app.chrome.theme = app.chrome.live_theme.resolve(&cfg.theme);
    app.chrome.animations = cfg.animations;
    crate::critters::apply(
        app,
        cfg.critters,
        cfg.critters_size_word(),
        cfg.critters_often_word(),
    );
    app.chrome.black_background = cfg.black_background;
    app.chrome.icons = crate::icons::IconStyle::from_setting(cfg.icons_word());
    if let Some(mode) = cfg.diff_mode() {
        app.modals.diff_mode = mode;
    }
    app.modals.diff_mode_configured = cfg.diff_mode().is_some();
    app.modals.diff_full_context = cfg.diff_context_word() == "full";
    app.launcher.card_issue_number = cfg.card_issue_number;
    app.launcher.show_all_worktrees = cfg.show_all_worktrees;
    app.launcher.auto_cleanup_merged = cfg.auto_cleanup_merged;
    app.launcher.hide_card_marks = cfg.hide_card_marks;
    app.launcher.highlight_current_card = cfg.highlight_current_card;
    crate::quick_prompt::mirror_accounts(app, cfg);
    app.launcher.launcher_pane_at = cfg.pane_side();
    app.launcher.launcher_list = cfg.list_layout();
    app.launcher.launcher_nested = cfg.nested_layout();
    app.launcher.launcher_columns = cfg.columns_layout();
    app.launcher.launcher_kanban = cfg.kanban;
    app.launcher.kanban_all = cfg.kanban_all();
    app.launcher.columns_hide_projects = cfg.hide_projects;
    app.launcher.columns_hide_worktrees = cfg.hide_worktrees;
    app.launcher.columns_hide_sessions = cfg.hide_sessions;
    app.launcher.columns_recent_prompts = cfg.recent_prompts;
    app.launcher.columns_recent_prompts_count = cfg.recent_prompts_count;
    app.launcher.launcher_all_open = cfg.expand_all_worktrees;
    set_hide_draft_prs(app, cfg.hide_draft_prs);
    if app.desk.subagents.hide_done != cfg.hide_done_subagents {
        app.desk.subagents.hide_done = cfg.hide_done_subagents;
        super::desk::refresh_subagents(app);
    }
}

/// `R` in the settings overlay, confirmed: rewrite config.json from the
/// defaults (removing config.local.json), adopt them live (values and
/// hotkeys both), and put the overlay back where it was so the reset
/// values are the next thing on screen.
pub(crate) fn reset_settings(app: &mut App) {
    let result = crate::config::Config::reset_to_defaults();
    reopen_settings(app);
    match result {
        Ok(cfg) => {
            apply_config(app, &cfg);
            app.chrome.keymap = cfg.keymap();
            // Every mirrored value was just adopted from the reset, so
            // nothing the files held before it is still pending.
            app.chrome.config_watch.arm();
            if let Some(view) = settings_mut(app) {
                view.info("every setting is back to its default");
            }
        }
        Err(err) => app.chrome.flash = Some(format!("couldn't reset settings: {err}")),
    }
}

/// Open the settings overlay from the panels. The remembered tab / row /
/// strip-vs-list is restored only while it's fresh: closed more than
/// [`crate::app::SETTINGS_MEMORY_TTL`] ago, it's forgotten and the overlay
/// comes up like a first open — first tab, top row, cursor on the strip.
pub(crate) fn open_settings(app: &mut App) {
    if app.settings_memory_expired() {
        app.forget_settings_focus();
    }
    reopen_settings(app);
}

/// Swap a session picker for the settings overlay parked on that
/// harness's Agents section, cursor on its Enabled row. The remembered
/// tab and row update too, so an Esc-then-`s` lands back where `?` left.
pub(crate) fn open_harness_settings(app: &mut App, kind: AgentKind, custom: Option<String>) {
    use crate::config::{agents_tab, locate_agent, HarnessField};
    let id = match kind {
        AgentKind::Custom => custom.unwrap_or_default(),
        _ => kind.as_str().to_string(),
    };
    let tab = agents_tab();
    app.modals.settings_tab = tab;
    if let Some((_, row)) = locate_agent(&id, HarnessField::Enabled) {
        if let Some(slot) = app.modals.settings_selected.get_mut(tab) {
            *slot = row;
        }
    }
    app.modals.settings_on_tabs = false;
    app.modals.overlay = Some(Overlay::Settings(SettingsView::new(
        tab,
        app.settings_row(tab),
        false,
    )));
}

/// Put the settings overlay back up on its remembered tab and row, no
/// questions asked. `open_settings` is the from-the-panels entry that
/// checks the memory's age first; this one is for mid-visit round trips
/// (the reset confirmation) where the position can't have gone stale.
pub(crate) fn reopen_settings(app: &mut App) {
    let tab = app.modals.settings_tab;
    app.modals.overlay = Some(Overlay::Settings(SettingsView::new(
        tab,
        app.settings_row(tab),
        app.modals.settings_on_tabs,
    )));
}

/// Take the settings overlay down and start the clock on its remembered
/// position (see `open_settings`). Both ways out — Esc/`q`/`s` and a click
/// outside the modal — go through here.
pub(crate) fn close_settings(app: &mut App) {
    app.modals.overlay = None;
    app.note_settings_closed();
}

/// Show or hide the drafts in the PROJECT OPEN PRS GROUP and `/` (Settings
/// → Appearance, or the panel menu's **Hide draft PRs**). Nothing stored
/// changes — `listed_open_prs` reads the flag — but the rows under the
/// cursor do, so it follows its pull request by URL, as
/// `reconcile_open_pr_cursor` does after a refresh. A cursor on a draft
/// that just went has nothing to follow: it lands on the nearest row left
/// and the pane is told to read whatever that is. True when that landing
/// was a checkout, so a caller with a request buffer can bring its session
/// up (`toggle_hide_draft_prs`); `apply_config` has none, and there the
/// pane catches up on the next move, as it does after the ROOT WORKTREE
/// toggle.
pub(crate) fn set_hide_draft_prs(app: &mut App, hidden: bool) -> bool {
    if app.launcher.hide_draft_prs == hidden {
        return false;
    }
    let was = app.selected_worktree_pr().cloned();
    let checkout = app.selected_worktree().map(|w| w.id.clone());
    app.launcher.hide_draft_prs = hidden;
    app.chrome.dirty = true;
    refresh_palette(app);
    // A checkout on a draft's branch nests under it while the draft is
    // listed and is a plain row while it is not: either way it stays
    // under the cursor.
    follow_checkout(app, checkout.as_ref());
    let Some(was) = was else {
        return false;
    };
    if let Some(i) = app.open_pr_row_of(&was.url) {
        app.nav.sel_worktree = i;
        return false;
    }
    let last = app.worktree_row_count().saturating_sub(1);
    app.nav.sel_worktree = app.nav.sel_worktree.min(last);
    schedule_pr_detail(app);
    app.selected_worktree().is_some()
}

/// **Hide draft PRs** / **Show draft PRs** on the Worktrees panel menu:
/// flip the `hide_draft_prs` SETTING where the group is and write it to
/// CONFIG.JSON — the file is where the choice persists, the app field is
/// the live copy. A cursor
/// that was on a draft and landed on a checkout gets that checkout's
/// session brought up, as a fold does (`toggle_open_prs`): the PTY
/// underneath was deliberately left attached while the cursor was in the
/// group, and it may belong to another worktree.
pub(crate) fn toggle_hide_draft_prs(app: &mut App, out: &mut Vec<ClientRequest>) {
    let mut cfg = crate::config::Config::load();
    cfg.hide_draft_prs = !app.launcher.hide_draft_prs;
    if !save_config(app, &cfg) {
        return;
    }
    if set_hide_draft_prs(app, cfg.hide_draft_prs) {
        restore_session(app, out);
        fire_pending_attach(app, out);
    }
    app.chrome.flash = Some(if cfg.hide_draft_prs {
        "draft pull requests hidden (Settings → Appearance)".into()
    } else {
        "draft pull requests shown".into()
    });
}
