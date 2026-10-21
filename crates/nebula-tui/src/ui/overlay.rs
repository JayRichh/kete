use super::*;
use crate::config::ConfigExt;

pub(super) fn draw_overlay(f: &mut Frame, app: &mut App) {
    let th = app.chrome.theme;
    let Some(overlay) = app.modals.overlay.clone() else {
        return;
    };
    // A box opened from the ISSUES MODAL or the PULL REQUESTS MODAL stands
    // on it rather than taking it away: the modal is the bottom layer, the
    // box — and any picker the box has up — is drawn over it.
    use crate::quick_prompt::ModalUnder;
    match crate::quick_prompt::modal_under(&overlay) {
        Some(ModalUnder::Issues(view)) => crate::issues::draw(f, app, &view, th, true),
        Some(ModalUnder::PullRequests(view)) => crate::pr_modal::draw(f, app, &view, th, true),
        None => {}
    }
    // A menu opened over an overlay (`?`, or a right-click on one of its
    // rows) stands on it: the diff, the tree or the accounts stay on
    // screen under the menu, so the row it was opened on is still seen.
    if matches!(overlay, Overlay::Menu(_)) {
        if let Some(under) = app.modals.verbs_under.as_deref().cloned() {
            under.draw(f, app);
        }
    }
    overlay.draw(f, app);
}

impl Overlay {
    fn draw(self, f: &mut Frame, app: &mut App) {
        match self {
            Overlay::ProjectPicker(picker) => draw_project_picker_overlay(f, app, picker),
            Overlay::Menu(menu) => draw_menu_overlay(f, app, menu),
            Overlay::Confirm(confirm) => draw_confirm_overlay(f, app, confirm),
            Overlay::Prompt(prompt) if prompt.is_multiline() => {
                draw_multiline_prompt_overlay(f, app, prompt)
            }
            Overlay::Prompt(prompt) => draw_prompt_overlay(f, app, prompt),
            Overlay::Help(view) => draw_help_overlay(f, app, view),
            Overlay::Settings(view) => draw_settings_overlay(f, app, view),
            Overlay::Metrics(view) => draw_metrics_overlay(f, app, view),
            Overlay::Diff(view) => draw_diff_overlay(f, app, view),
            Overlay::Palette(palette) => draw_palette_overlay(f, app, palette),
            Overlay::Files(finder) => draw_files_overlay(f, app, finder),
            Overlay::Grep(view) => draw_grep_overlay(f, app, view),
            Overlay::Hosts(view) => draw_hosts_overlay(f, app, view),
            Overlay::AgentPresets(view) => draw_agent_presets_overlay(f, app, view),
            Overlay::AgentPresetEditor(editor) => draw_agent_preset_editor_overlay(f, app, editor),
            Overlay::Issues(view) => draw_issues_overlay(f, app, view),
            Overlay::PullRequests(view) => draw_pull_requests_overlay(f, app, view),
            Overlay::BranchSwitch(view) => draw_branch_switch_overlay(f, app, view),
            Overlay::FileTabs(view) => draw_file_tabs_overlay(f, app, view),
            Overlay::Tree(view) => draw_tree_overlay(f, app, view),
            Overlay::Review(view) => draw_review_overlay(f, app, view),
            Overlay::OrchestratorStart(view) => draw_orchestrator_start_overlay(f, app, view),
            Overlay::Accounts(view) => super::accounts_view::draw(f, app, view),
            Overlay::Subagents(view) => super::subagents_view::draw(f, app, view),
            Overlay::Ado(view) => super::ado_view::draw(f, app, view),
            Overlay::Ship(view) => super::ship_view::draw(f, app, view),
            Overlay::CritterLog(mut view) => {
                let area = f.area();
                crate::critters::draw_log(f.buffer_mut(), app, &mut view, area);
                if let Some(Overlay::CritterLog(live)) = &mut app.modals.overlay {
                    live.area = view.area;
                }
            }
            // 0.60: the board and gauntlet lanes' (`ui::studio_board`,
            // `ui::gauntlet_view`), drawn over the whole frame.
            Overlay::StudioBoard(_) => {
                let area = f.area();
                super::studio_board::draw(f, app, area);
            }
            Overlay::Gauntlet(_) => {
                let area = f.area();
                super::gauntlet_view::draw(f, app, area);
            }
            Overlay::StateMap(_) => crate::state_overlay::draw_live(f, app),
        }
    }
}

fn draw_review_overlay(f: &mut Frame, app: &mut App, view: crate::review_modal::ReviewView) {
    super::review_view::draw(f, app, view);
}

fn draw_orchestrator_start_overlay(
    f: &mut Frame,
    app: &mut App,
    view: crate::orchestrator_view::StartView,
) {
    super::review_view::draw_start(f, app, view);
}

fn draw_project_picker_overlay(
    f: &mut Frame,
    app: &mut App,
    picker: crate::launcher::ProjectPicker,
) {
    // `^P` layers the project list over the box rather than
    // taking the box away: the task you typed is still in front
    // of you while you pick where it lands.
    if launcher_view::picker_over_box(app, &picker) {
        let box_behind = crate::quick_prompt::backdrop_box(&picker.back);
        draw_multiline_prompt(f, app, &box_behind, true);
    }
    launcher_view::draw_project_picker(f, app, &picker)
}

fn draw_menu_overlay(f: &mut Frame, app: &mut App, menu: crate::app::ContextMenu) {
    let th = app.chrome.theme;
    // `Tab` (the harness) and `^O` (the model) layer their list
    // over the box rather than taking the box away, as `^P` does:
    // the task you typed is still in front of you while you pick
    // what will run it.
    let backdrop = draw_menu_backdrop(f, app, &menu);
    let over = backdrop.map(|(rect, _)| rect);
    // The WORKTREE PICKER hangs from the branch it was opened on,
    // wherever the box has it this frame — a resize moves both —
    // its rows' text in the branch's column (a border and a space
    // in). Centered over the box when the details row found no
    // room for the branch.
    let at = menu.at.or_else(|| {
        let (_, branch) = backdrop.filter(|_| menu.is_launch_worktree_picker())?;
        (branch.width > 0).then(|| (branch.x.saturating_sub(2), branch.y + 1))
    });
    // A type-ahead submenu shows its query in the title: `Cursor
    // model ⌕ opus`, the bare ⌕ while nothing is typed yet. A
    // Claude list `Tab` sent to the cloud says so there as well —
    // `Claude model · cloud ⌕` — its rows being model names.
    let cloud = if menu.lists_claude_cloud() {
        crate::app::CLOUD_LABEL
    } else {
        ""
    };
    let title_text = menu.title.as_deref().map(|t| match &menu.filter {
        Some(f) if !f.query.is_empty() => format!("{t}{cloud} ⌕ {}", f.query),
        Some(_) => format!("{t}{cloud} ⌕"),
        None => format!("{t}{cloud}"),
    });
    let title_width = title_text
        .as_deref()
        .map(|t| t.chars().count() + 2)
        .unwrap_or(0);
    // An editor row leads with its editor's glyph (VS Code's, Visual
    // Studio's; `VSC` / `VS` without a Nerd Font).
    let glyphs: Vec<Option<(String, ratatui::style::Color)>> = menu
        .items
        .iter()
        .map(|i| crate::editors::glyph(i, app.chrome.icons))
        .collect();
    let glyph_w = |g: &Option<(String, ratatui::style::Color)>| {
        g.as_ref().map_or(0, |(t, _)| t.chars().count())
    };
    let label_w = menu
        .items
        .iter()
        .zip(&glyphs)
        .map(|(i, g)| i.label.chars().count() + glyph_w(g))
        .max()
        .unwrap_or(8);
    // Each verb's key at the row's right, as the keymap has it now (a
    // rebound key shows as bound, an unbound one shows nothing): the
    // menu teaches the key while the mouse runs the verb.
    let keys: Vec<String> = menu.items.iter().map(|i| menu_key(app, i)).collect();
    let key_w = keys.iter().map(|k| k.chars().count()).max().unwrap_or(0);
    let key_col = if key_w > 0 { key_w + 2 } else { 0 };
    // Rows that expand into a submenu get a right-aligned ▸ in an
    // extra column so the affordance is visible before hovering.
    let any_submenu = menu.items.iter().any(|i| i.action.submenu().is_some());
    // The modal is as wide as its rows or its title, whichever is
    // longer, and no wider: its keys go in the footer bar (see
    // `menu_footer_hint`), not in the bottom border, so a hint
    // that outgrows the rows — the pickers' `Tab: cloud off  s/?:
    // settings` did, doubling the width of a six-row list — never
    // pads the modal with empty space, and hovering a row with more
    // keys (the Claude row's Tab) never resizes it.
    let width = (label_w + 4 + key_col + if any_submenu { 2 } else { 0 })
        .max(title_width + 2)
        .min(f.area().width as usize) as u16;
    let height = menu.items.len() as u16 + 2;
    let area = match at {
        Some((ax, ay)) => {
            let x = ax.min(f.area().width.saturating_sub(width));
            let y = if ay + height > f.area().height {
                ay.saturating_sub(height)
            } else {
                ay
            };
            Rect {
                x,
                y,
                width,
                height: height.min(f.area().height),
            }
        }
        None => over_box_rect(f.area(), over, width, height),
    };
    f.render_widget(Clear, area);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(th.accent));
    if let Some(title) = &title_text {
        block = block.title(Span::styled(
            format!(" {title} "),
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
        ));
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Every row spans the modal — the hovered row's bar reaches
    // the border, and the ▸ sits at the right edge — so a title
    // wider than the rows leaves no ragged gap beside them.
    let row_w = inner.width as usize;
    let label_w = row_w.saturating_sub(if any_submenu { 4 } else { 2 } + key_col);
    // A list taller than the screen scrolls with its hover, only as far
    // as the hover needs: a row lit by the pointer stays under it.
    let skip = crate::app::menu_window(
        menu.scroll,
        menu.hover,
        menu.items.len(),
        inner.height as usize,
    );
    for (i, item) in menu.items.iter().enumerate().skip(skip) {
        let Some(row) = row_rect(inner, i - skip) else {
            break;
        };
        let mut style = if item.destructive {
            Style::default().fg(th.err)
        } else {
            Style::default()
        };
        if i == menu.hover {
            style = style.bg(th.sel_bg).add_modifier(Modifier::BOLD);
        }
        let glyph = &glyphs[i];
        let room = label_w.saturating_sub(glyph_w(glyph));
        let label: String = item.label.chars().take(room).collect();
        let key = if key_col > 0 {
            format!("{:>key_w$}  ", keys[i])
        } else {
            String::new()
        };
        let (text, tail) = if item.action.submenu().is_some() {
            (format!("{label:<room$}"), format!("{key}▸ "))
        } else if any_submenu {
            (format!("{label:<room$}"), format!("{key}  "))
        } else {
            (format!("{label:<room$}"), key)
        };
        let dim = if i == menu.hover {
            style
        } else {
            style.fg(th.dim)
        };
        let tail = format!(" {tail}");
        let mut spans = vec![Span::styled(" ", style)];
        if let Some((g, color)) = glyph {
            spans.push(Span::styled(g.clone(), style.fg(*color)));
        }
        spans.push(Span::styled(text, style));
        spans.push(Span::styled(tail, dim));
        f.render_widget(Paragraph::new(Line::from(spans)), row);
    }
    // Record the drawn area and window for the pointer's hit test.
    if let Some(Overlay::Menu(m)) = &mut app.modals.overlay {
        m.area = area;
        m.scroll = skip;
    }
}

/// The key a menu row shows: its registry verb's first chord as bound
/// now, else a key of its own; empty for a row that has neither.
fn menu_key(app: &App, item: &crate::app::MenuItem) -> String {
    match (&item.verb, &item.key) {
        (Some(verb), _) => app
            .chrome
            .keymap
            .first(*verb)
            .map(|c| c.display())
            .unwrap_or_default(),
        (None, Some(key)) => key.clone(),
        (None, None) => String::new(),
    }
}

fn draw_confirm_overlay(f: &mut Frame, app: &mut App, confirm: crate::app::ConfirmDialog) {
    let th = app.chrome.theme;
    // Bulk deletes itemize their casualties across several message
    // lines — size the dialog to fit them.
    let msg_lines: Vec<&str> = confirm.message.lines().collect();
    // A delete that empties a linked worktree asks about the
    // checkout in the same dialog, so its legend has three
    // answers: yes takes both, no takes the card alone, and
    // cancel keeps the card alive. The dialog is sized to the
    // legend too, so the three never wrap.
    // Each answer is a BUTTON (`ConfirmDialog::answers`): a click on it is
    // its key. The one under the pointer is underlined and brightened, as
    // the grid's buttons are; the destructive answers keep the red.
    let hover = app.modals.button_hover;
    let drawn = confirm.answer_rects();
    let mut legend_spans = Vec::new();
    for (i, answer) in confirm.answers().into_iter().enumerate() {
        if i > 0 {
            legend_spans.push(Span::raw(
                " ".repeat(usize::from(crate::app::CONFIRM_ANSWER_GAP)),
            ));
        }
        let mut style = Style::default().fg(if answer.destructive { th.err } else { th.dim });
        let lit = drawn.iter().any(|(r, a)| *a == answer && Some(*r) == hover);
        if lit {
            style = style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
            if !answer.destructive {
                style = style.fg(th.text);
            }
        }
        legend_spans.push(Span::styled(answer.label, style));
    }
    let legend = Line::from(legend_spans);
    let longest = msg_lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(legend.width());
    let width = (longest as u16 + 4).max(CONFIRM_MIN_W);
    let height = msg_lines.len() as u16 + 4;
    let area = centered_rect(f.area(), width, height);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(th.err))
        .title(Span::styled(
            format!(" {} ", confirm.title),
            Style::default().fg(th.err),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line> = msg_lines
        .into_iter()
        .map(|l| Line::from(l.to_string()))
        .collect();
    lines.push(Line::from(""));
    lines.push(legend);
    f.render_widget(Paragraph::new(lines), inner);
    // Record the drawn area for click hit-testing.
    if let Some(Overlay::Confirm(c)) = &mut app.modals.overlay {
        c.area = area;
    }
}

fn draw_multiline_prompt_overlay(f: &mut Frame, app: &mut App, prompt: PromptDialog) {
    draw_multiline_prompt(f, app, &prompt, false);
}

fn draw_prompt_overlay(f: &mut Frame, app: &mut App, prompt: PromptDialog) {
    let th = app.chrome.theme;
    // Path prompts get a wide dialog with the live directory
    // listing between the input and the hint; the dialog grows to
    // fit the listing (at least one row, for the empty message).
    let is_path = prompt.completes_paths();
    let width = if is_path { PATH_PROMPT_W } else { PROMPT_W };
    let list_h = if is_path {
        prompt.dirs.len().clamp(1, 8) as u16
    } else {
        0
    };
    let area = centered_rect(f.area(), width, 6 + list_h);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(th.accent))
        .title(Span::styled(
            format!(" {} ", prompt.title),
            Style::default().fg(th.accent),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Row 0: the label, with the listing size tucked after it.
    if let Some(r) = row_rect(inner, 0) {
        let mut spans = vec![Span::styled(
            prompt.label.clone(),
            Style::default().fg(th.dim),
        )];
        if prompt.dirs.len() > list_h as usize {
            spans.push(Span::styled(
                format!("  ·  {} dirs", prompt.dirs.len()),
                Style::default().fg(th.dim),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }

    // Row 1: the input. Long paths scroll under it around the
    // caret; the caret dims while a listing row is highlighted
    // (Enter takes the highlight, not the text).
    if let Some(r) = row_rect(inner, 1) {
        let budget = inner.width.saturating_sub(2) as usize;
        let cursor = if prompt.hover.is_some() {
            th.dim
        } else {
            th.text
        };
        let mut spans = vec![Span::raw("> ")];
        spans.extend(input_spans(&prompt.input, budget, cursor, th));
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }

    // The listing: one raised-fill row per directory, a ● on git
    // repos, the typed partial lit like a fuzzy match. A stateless
    // follow-window keeps the highlighted row visible.
    let mut list_area = Rect::default();
    if is_path {
        list_area = Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: list_h.min(inner.height.saturating_sub(2)),
        };
        if prompt.dirs.is_empty() {
            if let Some(r) = row_rect(list_area, 0) {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        "  no matching directories",
                        Style::default().fg(th.dim),
                    )),
                    r,
                );
            }
        }
        let (_, partial) = crate::completion::split_input(&prompt.input);
        let hit = partial.chars().count();
        let start = prompt.window_start(list_area.height as usize);
        for (row, (i, entry)) in prompt.dirs.iter().enumerate().skip(start).enumerate() {
            let Some(r) = row_rect(list_area, row) else {
                break;
            };
            let marker = if entry.is_repo {
                Span::styled("● ", Style::default().fg(th.ok))
            } else {
                Span::styled("· ", Style::default().fg(th.dim))
            };
            let budget = (inner.width as usize).saturating_sub(5);
            let shown = truncate(&entry.name, budget);
            let positions: Vec<usize> = (0..hit.min(shown.chars().count())).collect();
            let mut spans = vec![Span::raw(" "), marker];
            spans.extend(fuzzy_highlight_spans(&shown, &positions, th));
            spans.push(Span::styled("/", Style::default().fg(th.dim)));
            render_row(f, r, spans, prompt.hover == Some(i), true, th);
        }
    }

    // Bottom row: the key hints, `[Enter]` and `[Esc] cancel` at either end
    // of it BUTTONS (`PromptDialog::answer_rects`) — a click on one is the
    // key — in the brighter text a button is drawn in, underlined under
    // the pointer.
    if let Some(r) = row_rect(inner, (3 + list_h) as usize) {
        let hint = prompt.key_row();
        let enter = hint.split("  ").next().unwrap_or_default();
        let esc = "[Esc] cancel";
        let middle = &hint[enter.len()..hint.len() - esc.len()];
        let hovered = |key: crossterm::event::KeyCode| {
            prompt
                .answer_rects()
                .iter()
                .any(|(rect, k)| *k == key && Some(*rect) == app.modals.button_hover)
        };
        let button = |label: &'static str, lit: bool| {
            let style = Style::default().fg(th.text);
            Span::styled(
                label,
                if lit {
                    style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
                } else {
                    style
                },
            )
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                button(enter, hovered(crossterm::event::KeyCode::Enter)),
                Span::styled(middle, Style::default().fg(th.dim)),
                button(esc, hovered(crossterm::event::KeyCode::Esc)),
            ])),
            r,
        );
    }
    // Record the listing and dialog rects for click hit-testing.
    if let Some(Overlay::Prompt(p)) = &mut app.modals.overlay {
        p.list_area = list_area;
        p.area = area;
    }
}

use crate::keymap::Action::*;
enum HelpKeys {
    Lit(&'static str),
    Act(&'static [crate::keymap::Action]),
}
use HelpKeys::{Act, Lit};
type HelpSection = (&'static str, &'static [(HelpKeys, &'static str)]);
const HELP_LEFT: &[HelpSection] = &[
    (
        "NAVIGATE & SEARCH",
        &[
            (Act(&[MoveDown, MoveUp]), "walk the cards (2×: tabs)"),
            (Act(&[FocusLeft, FocusRight]), "step along a row of cards"),
            (Act(&[FocusNext]), "open / fold the checkout"),
            (Act(&[Activate]), "into the pane (attach)"),
            (Act(&[HalfPageDown, HalfPageUp]), "jump two rows of cards"),
            (
                Act(&[NextProjectTab, PrevProjectTab]),
                "next / previous project tab",
            ),
            (Lit("1-9"), "open that project tab"),
            (
                Act(&[ProjectDropdown, CloseProjectTab]),
                "project list / close tab",
            ),
            (Act(&[AddProject]), "open a folder as a project"),
            (
                Act(&[Palette, CommandPalette]),
                "jump to anything / commands",
            ),
            (Lit("^o / ^f"), "jump pick: open / focus row"),
            (
                Act(&[NextAttention, PrevAttention]),
                "next/prev session needing you",
            ),
            (Act(&[NavBack, NavForward]), "back / forward a place"),
            (Act(&[ReorderUp, ReorderDown]), "move card / band / tab"),
            (
                Act(&[MoveCardLeft, MoveCardRight]),
                "board: card to the next lane",
            ),
            (Act(&[FindFile, Grep]), "find file / in files (git grep)"),
            (Act(&[TreeBrowser]), "file tree browser"),
        ],
    ),
    (
        "CHECKOUTS & GITHUB",
        &[
            (Act(&[OpenWorktree]), "open in editor (open command)"),
            (
                Act(&[OpenInBrowser, Screenshot]),
                "localhost: browser / screenshot",
            ),
            (Act(&[GitDiff]), "diff (^r reviewed, ^t tree)"),
            (Act(&[OpenRepo]), "the repo on GitHub"),
            (
                Act(&[OpenPullRequest, OpenIssue]),
                "card's PR / issue on GitHub",
            ),
            (Act(&[RefreshPullRequests]), "reload PRs + issues (GitHub)"),
            (Act(&[Issues, PullRequests]), "issues / pull requests"),
            (Act(&[Ado]), "Azure DevOps: tasks, PRs, branches"),
            (Act(&[CommentPullRequest]), "comment on the card's PR"),
            (Act(&[SwitchBranch]), "switch the ⌂ root's branch"),
        ],
    ),
    (
        // Every typed field — names, filters, queries — is the
        // same line editor (text_input.rs).
        "TYPING IN A FIELD",
        &[
            (Lit("←→ / ⌥←→"), "move by character / by word"),
            (Lit("^a^e ⌥⌫ ^u^k"), "ends · del word · kill line"),
        ],
    ),
];
const HELP_RIGHT: &[HelpSection] = &[
    (
        "SESSIONS",
        &[
            (Act(&[QuickPrompt]), "quick prompt: Enter launches"),
            (Act(&[New]), "new session: pick a CLI first"),
            (
                Act(&[DuplicateSession, MoveSession]),
                "this card: re-prompt / move",
            ),
            (Act(&[AgentPresets]), "agent presets: saved launches"),
            (
                Act(&[NewTerminal, OpenGhosttyTab]),
                "terminal: here / in Ghostty",
            ),
            (Act(&[FollowUp, Rename]), "follow-up prompt / rename"),
            (Act(&[Subagents]), "subagents and session tree"),
            (
                Act(&[Archive, Unarchive, ToggleArchived]),
                "archive / unarchive / show",
            ),
            (Act(&[Delete, DeleteAll]), "delete one / delete all"),
        ],
    ),
    (
        "TERMINAL & MOUSE",
        &[
            (Act(&[Activate]), "lock input"),
            (Act(&[UnlockTerminal]), "unlock, back to the card"),
            (Act(&[PaneTabs]), "pane: session ↔ its terminals"),
            (Lit("drag"), "select + copy (2×click: word)"),
            (Lit("click / drag"), "the app that took the mouse"),
            (Lit("⌥click"), "open URL / file under cursor"),
            (Lit("⇧drag"), "select via your terminal"),
            (Lit("right-click"), "card / tab menu: run, restart"),
            (Lit("drag the pane edge"), "resize the pane"),
            (Lit("click outside"), "dismiss any modal (= Esc)"),
        ],
    ),
    (
        "GENERAL",
        &[
            (Act(&[ToggleLauncherPane]), "fold / unfold the pane"),
            (Act(&[ToggleFullScreen]), "full-screen / normal size"),
            (
                Act(&[ToggleKanban, ToggleOrchestratorView]),
                "board / orchestrator view",
            ),
            // The SHIFT PAIRS' rule (#93), once, for every
            // letter above that has a shifted twin.
            (Lit("⇧ + letter"), "bigger, or outside kete"),
            (Act(&[Hosts]), "ssh hosts (a: new, d: del)"),
            (Act(&[Settings]), "settings; Hotkeys tab rebinds"),
            (Act(&[Metrics, Accounts]), "memory / accounts + usage"),
            (Act(&[Verbs]), "what the selection can do"),
            (Act(&[Quit, Help]), "quit / toggle this help"),
        ],
    ),
];

fn draw_help_overlay(f: &mut Frame, app: &mut App, view: crate::app::HelpView) {
    if view.whats_new {
        draw_whats_new(f, app, view);
        return;
    }
    let th = app.chrome.theme;
    // Grouped keymap in two columns: reads by task instead of one
    // giant list, and at ~24 rows it fits a stock terminal window
    // (the old single list clipped its tail on short screens).
    // Key columns come from the live keymap, not hardcoded text:
    // every one of these is rebindable in Settings → Hotkeys, and
    // help that lies about that is worse than no help. Literals
    // are for keys that belong to an overlay rather than the
    // grid, which is why they aren't rebindable.
    // What to print in the key column: a literal, or every chord
    // each action currently answers to but the ⌘ aliases
    // (`Keymap::shown_chords`).
    // An action bound to more chords than the key column holds —
    // open's ⇧Enter ⇧O ⌥Enter — loses whole chords off the end
    // and gains an ellipsis, never a cut mid-chord; the Hotkeys
    // tab lists every one.
    let keys_of = |k: &HelpKeys| -> String {
        match k {
            Lit(s) => (*s).to_string(),
            Act(actions) => {
                let full = actions
                    .iter()
                    .map(|a| app.chrome.keymap.shown_label(*a))
                    .collect::<Vec<_>>()
                    .join(" / ");
                if actions.len() != 1 || full.chars().count() <= HELP_KEY_W {
                    return full;
                }
                let chords: Vec<String> = app
                    .chrome
                    .keymap
                    .shown_chords(actions[0])
                    .iter()
                    .map(|c| c.display().to_string())
                    .collect();
                (1..chords.len())
                    .rev()
                    .map(|kept| format!("{} …", chords[..kept].join(" ")))
                    .find(|shown| shown.chars().count() <= HELP_KEY_W)
                    .unwrap_or(full)
            }
        }
    };
    // Rows a column needs: each section is a header plus its
    // entries, with a blank line between sections.
    let rows = |sections: &[HelpSection]| -> u16 {
        sections
            .iter()
            .map(|(_, entries)| entries.len() as u16 + 1)
            .sum::<u16>()
            + sections.len().saturating_sub(1) as u16
    };
    let height = rows(HELP_LEFT).max(rows(HELP_RIGHT)) + 2;
    let area = centered_rect(f.area(), HELP_W, height);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(th.accent))
        .title(" Help ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    // The WHAT'S NEW page, a key and a click away on the top border.
    let wn_label = " w: what's new ";
    let wn_w = (wn_label.chars().count() as u16).min(area.width.saturating_sub(12));
    let wn_rect = Rect {
        x: area.right().saturating_sub(wn_w + 2),
        y: area.y,
        width: wn_w,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Span::styled(wn_label, Style::default().fg(th.accent))),
        wn_rect,
    );
    let [left_a, right_a] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(inner);
    let hover = match &app.modals.overlay {
        Some(Overlay::Help(h)) => h.hover,
        _ => None,
    };
    // Every row naming an action is a button: a click on it closes the
    // help and runs the action, as its key would. A row of two or three
    // actions (`a / u / ⇧A`) is a button per key where the keys are drawn
    // whole, the row's first action everywhere else on it.
    let mut buttons: Vec<(Rect, crate::keymap::Action)> = Vec::new();
    let mut column = |sections: &[HelpSection], col: Rect| -> Vec<Line> {
        let width = col.width;
        let mut lines = Vec::new();
        for (i, (title, entries)) in sections.iter().enumerate() {
            if i > 0 {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                format!(" {title}"),
                Style::default().fg(th.muted).add_modifier(Modifier::BOLD),
            )));
            for (k, v) in *entries {
                // Rebindable chords vary in width, so the key
                // column is padded to a fixed width and clipped
                // there — an exotic binding can't shove the
                // descriptions out of alignment.
                let full = keys_of(k);
                let keys = truncate(&full, HELP_KEY_W);
                let y = col.y + lines.len() as u16;
                let row = Rect {
                    y,
                    height: 1,
                    ..col
                };
                // Only the rows the box has room for are buttons.
                let drawn = y < col.y + col.height;
                if let (Act(actions), true) = (k, drawn) {
                    if keys == full && actions.len() > 1 {
                        let mut x = col.x + 1;
                        for (n, action) in actions.iter().enumerate() {
                            let label = app.chrome.keymap.shown_label(*action);
                            let w = label.chars().count() as u16;
                            buttons.push((Rect { x, width: w, ..row }, *action));
                            x += w + if n + 1 < actions.len() { 3 } else { 0 };
                        }
                    }
                    buttons.push((row, actions[0]));
                }
                let lit = hover == Some(row) && matches!(k, Act(_));
                let bg = |s: Style| if lit { s.bg(th.sel_bg) } else { s };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {keys:<width$}", width = HELP_KEY_W),
                        bg(Style::default().fg(th.accent)),
                    ),
                    Span::styled(
                        format!(
                            "{:<w$}",
                            truncate(v, (width as usize).saturating_sub(16)),
                            w = (width as usize).saturating_sub(HELP_KEY_W + 1)
                        ),
                        bg(Style::default().fg(if lit { th.text } else { th.dim })),
                    ),
                ]));
            }
        }
        lines
    };
    let left_lines = column(HELP_LEFT, left_a);
    let right_lines = column(HELP_RIGHT, right_a);
    f.render_widget(Paragraph::new(left_lines), left_a);
    f.render_widget(Paragraph::new(right_lines), right_a);
    // Record the drawn area and the rows' buttons for click hit-testing.
    if let Some(Overlay::Help(h)) = &mut app.modals.overlay {
        h.area = area;
        h.buttons = buttons;
        h.buttons.push((wn_rect, Action::WhatsNew));
    }
}

/// The WHAT'S NEW page (`learn.rs`): the last few releases in a line or
/// four each, then the keys that changed, scrolled with j/k. Its foot
/// names the way to every key (a button, as the keys page's rows are).
fn draw_whats_new(f: &mut Frame, app: &mut App, view: crate::app::HelpView) {
    let th = app.chrome.theme;
    let screen = f.area();
    let width = HELP_W.min(screen.width.saturating_sub(2));
    let lines = crate::learn::whats_new_lines(width.saturating_sub(2), th);
    // Rows the wrapped page takes, so the box is no taller than it.
    let inner_w = usize::from(width.saturating_sub(2)).max(1);
    let rows: u16 = lines
        .iter()
        .map(|l| (l.width().max(1).div_ceil(inner_w)) as u16)
        .sum();
    let height = (rows + 4).min(screen.height.saturating_sub(2)).max(6);
    let area = centered_rect(screen, width, height);
    let releases = nebula_config::learn::WHATS_NEW;
    let span = match (releases.last(), releases.first()) {
        (Some(oldest), Some(newest)) => {
            format!(" What's new in {} to {} ", oldest.version, newest.version)
        }
        _ => " What's new ".to_string(),
    };
    let inner = render_modal_frame(f, area, &span, th);
    let [body, _, foot] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let max_scroll = rows.saturating_sub(body.height);
    let scroll = view.scroll.min(max_scroll);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        body,
    );
    // The foot: how to move, and every key as a button.
    let keys = key_hint(app, Action::Help);
    let more = if max_scroll > 0 { "j/k: scroll  " } else { "" };
    let lead = format!(" {more}Esc: close  ");
    let all = format!("{keys}: all keys");
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(lead.clone(), Style::default().fg(th.dim)),
            Span::styled(all.clone(), Style::default().fg(th.accent)),
        ])),
        foot,
    );
    let button = Rect {
        x: foot.x + lead.chars().count() as u16,
        width: (all.chars().count() as u16).min(foot.width),
        ..foot
    };
    if let Some(Overlay::Help(h)) = &mut app.modals.overlay {
        h.area = area;
        h.scroll = scroll;
        h.buttons = vec![(button, Action::Help)];
    }
}

fn draw_settings_overlay(f: &mut Frame, app: &mut App, view: crate::app::SettingsView) {
    let th = app.chrome.theme;
    // A tab strip over a scrolling list. Splitting the settings by
    // tab is what keeps the modal short enough for a stock 24-row
    // terminal now that the Hotkeys tab alone is forty rows.
    let cfg = crate::config::Config::load();
    let tab = view.tab;
    let rows = crate::config::settings_rows(tab);
    // The Project tab's rows are the selected project's: its
    // name and path head the tab, and its entry is what the
    // values read. No project (an empty tree) leaves them n/a.
    let project = app
        .selected_project()
        .map(|p| (p.name.clone(), p.repo_path.clone()));
    // Rows the modal spends on anything but settings: the tab
    // strip and its rule above the body, and a blank + hint +
    // keys + config path below it.
    const CHROME: u16 = 2 + 4;
    let want = rows.len() as u16 + CHROME + 2;
    let height = want.min(f.area().height.saturating_sub(2)).max(CHROME + 3);
    let area = centered_rect(f.area(), SETTINGS_W, height);
    let inner = render_modal_frame(f, area, " Settings ", th);

    let dim = Style::default().fg(th.dim);
    // ---- tab strip ----
    let (strip, hits) = tab_strip(
        inner.x,
        crate::config::SETTINGS_TABS.iter().map(|t| (t.title, None)),
        tab,
        view.on_tabs,
        th,
    );
    let mut lines: Vec<Line> = vec![Line::from(strip), strip_rule(inner.width, th)];

    // ---- body ----
    let body_h = inner.height.saturating_sub(CHROME).max(1) as usize;
    let first_row = append_settings_body_lines(
        &mut lines,
        app,
        &cfg,
        &rows,
        &view,
        tab,
        body_h,
        inner,
        project.as_ref(),
        th,
        dim,
    );

    // ---- footer: notice or hint, then the keys, then the file ----
    lines.push(Line::from(""));
    match &view.notice {
        Some((text, level)) => lines.push(Line::from(Span::styled(
            truncate(&format!(" {text}"), inner.width as usize),
            match level {
                crate::app::NoticeLevel::Warn => Style::default().fg(th.warn),
                crate::app::NoticeLevel::Info => Style::default().fg(th.muted),
            },
        ))),
        None => {
            // A row the config file has double-booked explains
            // itself in place of its usual hint — that's the more
            // urgent thing to say about it.
            let shadowed = view
                .is_hotkeys()
                .then(|| app.chrome.keymap.shadowed_by(view.selected))
                .filter(|names| !names.is_empty());
            match shadowed {
                Some(names) => lines.push(Line::from(Span::styled(
                    truncate(
                        &format!(
                            " ✗ this key also belongs to {}, whichever is listed first wins",
                            names.join(", ")
                        ),
                        inner.width as usize,
                    ),
                    Style::default().fg(th.warn),
                ))),
                None => {
                    let mut hint = crate::config::hint_at(tab, view.selected);
                    // The CRITTERS rows lead to the collection log.
                    if crate::critters::is_critters_row(tab, view.selected) {
                        hint.push_str("  ⇧L critter log");
                    }
                    lines.push(Line::from(Span::styled(
                        truncate(&format!(" {hint}"), inner.width as usize),
                        dim,
                    )));
                }
            }
        }
    }
    lines.push(Line::from(Span::styled(
        truncate(
            &format!(" {}", settings_keys_hint(&view)),
            inner.width as usize,
        ),
        dim,
    )));
    // The file's path, and on the same row the buttons that carry the
    // settings to a file and back (`kete config export / import`).
    let path = nebula_core::paths::config_path();
    // On the Appearance tab, Kete's own look is a button too (`⇧D`).
    let look = crate::config::is_appearance_tab(tab);
    let mut buttons = vec![("⇧E export", Some(false)), ("⇧I import", Some(true))];
    if look {
        buttons.insert(0, ("⇧D Kete look", None));
    }
    let room: usize = buttons.iter().map(|(l, _)| l.chars().count() + 3).sum();
    let mut io_hits = Vec::new();
    let mut look_hit = None;
    let mut last = vec![Span::styled(
        format!(
            "{:<w$}",
            truncate(
                &format!(" {}", path.display()),
                (inner.width as usize).saturating_sub(room)
            ),
            w = (inner.width as usize).saturating_sub(room)
        ),
        dim,
    )];
    if inner.width as usize > room + 8 {
        let mut x = inner.x + inner.width - room as u16;
        let row = inner.y + lines.len() as u16;
        for (label, import) in buttons {
            let w = label.chars().count() as u16 + 2;
            last.push(Span::raw(" "));
            last.push(Span::styled(
                format!("[{label}]"),
                Style::default().fg(th.accent),
            ));
            let hit = Rect {
                x: x + 1,
                y: row,
                width: w,
                height: 1,
            };
            match import {
                Some(import) => io_hits.push((hit, import)),
                None => look_hit = Some(hit),
            }
            x += w + 1;
        }
    }
    lines.push(Line::from(last));
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(Overlay::Settings(v)) = &mut app.modals.overlay {
        v.area = area;
        v.io_hits = io_hits;
        v.look_hit = look_hit;
        v.tab_hits = hits;
        v.first_row = first_row;
        v.body_area = Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: body_h as u16,
        };
    }
}

#[allow(clippy::too_many_arguments)]
fn append_settings_body_lines(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    cfg: &crate::config::Config,
    rows: &[crate::config::SettingsRow],
    view: &crate::app::SettingsView,
    tab: usize,
    body_h: usize,
    inner: Rect,
    project: Option<&(String, std::path::PathBuf)>,
    th: Theme,
    dim: Style,
) -> usize {
    // Same stateless follow-window the panels use, in row space: the selected
    // row stays on screen without any scroll state.
    let sel_row = rows
        .iter()
        .position(|r| r.index() == Some(view.selected))
        .unwrap_or(0);
    let first_row = (sel_row + 1).saturating_sub(body_h);
    let today = crate::config::today_days();
    let new_prefix = |kind: crate::config::SettingKind| {
        if kind.is_new(today) {
            crate::config::NEW_SETTING_PREFIX
        } else {
            ""
        }
    };
    let label_w = crate::config::tab_settings(tab)
        .iter()
        .map(|spec| new_prefix(spec.kind).len() + spec.label.chars().count() + 1)
        .fold(28, usize::max);
    let project_settings = project.map(|(_, path)| cfg.project(path));
    for row in rows.iter().skip(first_row).take(body_h) {
        append_settings_row(
            lines,
            app,
            cfg,
            row,
            view,
            tab,
            inner,
            project,
            project_settings.as_ref(),
            label_w,
            &new_prefix,
            th,
            dim,
        );
    }
    for _ in lines.len()..(body_h + 2) {
        lines.push(Line::from(""));
    }
    first_row
}

#[allow(clippy::too_many_arguments)]
fn append_settings_row(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    cfg: &crate::config::Config,
    row: &crate::config::SettingsRow,
    view: &crate::app::SettingsView,
    tab: usize,
    inner: Rect,
    project: Option<&(String, std::path::PathBuf)>,
    project_settings: Option<&crate::config::ProjectSettings>,
    label_w: usize,
    new_prefix: &impl Fn(crate::config::SettingKind) -> &'static str,
    th: Theme,
    dim: Style,
) {
    match row {
        crate::config::SettingsRow::Blank => lines.push(Line::from("")),
        crate::config::SettingsRow::Header(title) => lines.push(Line::from(Span::styled(
            format!(" {title}"),
            Style::default().fg(th.muted).add_modifier(Modifier::BOLD),
        ))),
        crate::config::SettingsRow::Project => match project {
            Some((name, path)) => {
                let name = format!(" {name}");
                let room = (inner.width as usize).saturating_sub(name.chars().count());
                lines.push(Line::from(vec![
                    Span::styled(
                        name,
                        Style::default().fg(th.muted).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(truncate(&format!("  {}", path.display()), room), dim),
                ]));
            }
            None => lines.push(Line::from(Span::styled(
                " no project selected, these rows are the selected project's",
                Style::default().fg(th.warn),
            ))),
        },
        crate::config::SettingsRow::Setting(i) => append_setting_value_row(
            lines,
            cfg,
            *i,
            view,
            tab,
            inner,
            project_settings,
            label_w,
            new_prefix,
            th,
        ),
        crate::config::SettingsRow::Hotkey(i) => append_hotkey_row(lines, app, *i, view, th),
    }
}

#[allow(clippy::too_many_arguments)]
fn append_setting_value_row(
    lines: &mut Vec<Line<'static>>,
    cfg: &crate::config::Config,
    i: usize,
    view: &crate::app::SettingsView,
    tab: usize,
    inner: Rect,
    project_settings: Option<&crate::config::ProjectSettings>,
    label_w: usize,
    new_prefix: &impl Fn(crate::config::SettingKind) -> &'static str,
    th: Theme,
) {
    let (label, value, prefix) = if tab == crate::config::agents_tab() {
        match crate::config::AGENTS_HEAD.get(i) {
            Some(spec) => (
                spec.label.to_string(),
                cfg.value_label(spec.kind),
                new_prefix(spec.kind),
            ),
            None => {
                let (id, field) = cfg
                    .agent_row(i)
                    .expect("settings_rows indexes the Agents tab's harness rows");
                (field.label().to_string(), cfg.agent_value(&id, field), "")
            }
        }
    } else if let Some(harness) = (tab == crate::config::project_tab())
        .then(|| cfg.project_account_row(i))
        .flatten()
    {
        // A PROJECT TAB **Default account** row: the project's account
        // for the harness, or the spread's pick when it names none.
        let value = project_settings
            .map(|s| {
                s.default_profile(&harness)
                    .unwrap_or_else(|| "none (the spread picks)".into())
            })
            .unwrap_or_else(|| "n/a".into());
        (harness, value, "")
    } else {
        let spec =
            crate::config::setting_at(tab, i).expect("settings_rows indexes this tab's settings");
        let value = if spec.kind.is_project() {
            project_settings
                .map(|s| s.value_label(spec.kind))
                .unwrap_or_else(|| "n/a".into())
        } else {
            cfg.value_label(spec.kind)
        };
        (spec.label.to_string(), value, new_prefix(spec.kind))
    };
    let selected = i == view.selected && !view.on_tabs;
    let mut label_style = Style::default();
    let mut value_style = Style::default().fg(th.accent);
    if selected {
        label_style = label_style.bg(th.sel_bg).add_modifier(Modifier::BOLD);
        value_style = value_style.bg(th.sel_bg).add_modifier(Modifier::BOLD);
    }
    let room = (inner.width as usize).saturating_sub(3 + label_w + 2);
    let mut prefix_style = Style::default().fg(th.ok);
    if selected {
        prefix_style = prefix_style.bg(th.sel_bg).add_modifier(Modifier::BOLD);
    }
    let label_room = label_w - prefix.len();
    lines.push(Line::from(vec![
        Span::styled("   ", label_style),
        Span::styled(prefix, prefix_style),
        Span::styled(format!("{label:<label_room$}"), label_style),
        Span::styled(format!("[{}]", truncate(&value, room)), value_style),
    ]));
}

fn append_hotkey_row(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    i: usize,
    view: &crate::app::SettingsView,
    th: Theme,
) {
    let spec = crate::keymap::spec_at(i).expect("settings_rows indexes the action table");
    let selected = i == view.selected && !view.on_tabs;
    let value = if selected && view.capturing() {
        "press a key…".to_string()
    } else {
        app.chrome.keymap.display_at(i)
    };
    let reach = app.chrome.keymap.reach_at(i);
    let ambiguous = app.chrome.keymap.is_ambiguous(i);
    let mut label_style = Style::default();
    let mut value_style = Style::default().fg(if reach.is_fine() && !ambiguous {
        th.accent
    } else {
        th.warn
    });
    if selected {
        label_style = label_style.bg(th.sel_bg).add_modifier(Modifier::BOLD);
        value_style = value_style.bg(th.sel_bg).add_modifier(Modifier::BOLD);
    }
    let flag = match (ambiguous, reach) {
        (true, _) | (_, crate::keymap::Reach::Blocked) => "✗",
        (_, crate::keymap::Reach::Risky) => "⚠",
        _ => " ",
    };
    lines.push(Line::from(vec![
        Span::styled(format!("   {:<28}", spec.label), label_style),
        Span::styled(format!("{value:<18}"), value_style),
        Span::styled(flag.to_string(), Style::default().fg(th.warn)),
    ]));
}

struct MetricsRow {
    name: String,
    context: String,
    /// None = a group header, which is no one process.
    pid: Option<u32>,
    procs: u32,
    bytes: u64,
    /// None = not openable: nebula's own processes, a group header, or a
    /// pool spare (nothing to open until a CreateAgent adopts it).
    sref: Option<SessionRef>,
}

type MetricsKindTotals = std::collections::BTreeMap<&'static str, (u32, u32, u64)>;

fn metrics_rows(
    app: &App,
    view: &crate::app::MetricsView,
) -> (Vec<MetricsRow>, MetricsKindTotals, u64) {
    let mut rows: Vec<MetricsRow> = Vec::new();
    let mut spares: Vec<MetricsRow> = Vec::new();
    let mut kinds: MetricsKindTotals = std::collections::BTreeMap::new();
    let mut sessions_total: u64 = 0;
    let wt_context = |wt_id: &nebula_core::WorktreeId| -> String {
        app.tree
            .worktrees
            .iter()
            .find(|w| &w.id == wt_id)
            .map(|w| {
                let project = app
                    .tree
                    .projects
                    .iter()
                    .find(|p| p.id == w.project_id)
                    .map(|p| p.name.as_str())
                    .unwrap_or("?");
                format!("{project}/{}", w.branch)
            })
            .unwrap_or_default()
    };

    let Some(snap) = &view.snapshot else {
        return (rows, kinds, sessions_total);
    };
    for m in &snap.sessions {
        if let (SessionRef::Agent(_), Some(home)) = (&m.session, &m.prewarm) {
            let model = home
                .model
                .as_deref()
                .map(|model| format!(" · {model}"))
                .unwrap_or_default();
            let entry = kinds.entry("warm").or_default();
            entry.0 += 1;
            entry.1 += m.procs;
            entry.2 += m.rss_bytes;
            sessions_total += m.rss_bytes;
            spares.push(MetricsRow {
                name: format!("{}{model}", home.kind.as_str()),
                context: wt_context(&home.worktree),
                pid: Some(m.pid),
                procs: m.procs,
                bytes: m.rss_bytes,
                sref: None,
            });
            continue;
        }
        let (name, context, kind) = match &m.session {
            SessionRef::Agent(id) => {
                let agent = app.tree.agents.iter().find(|a| &a.id == id);
                let name = agent
                    .map(|a| format!("{} ({})", a.name, a.kind.as_str()))
                    .unwrap_or_else(|| "(unknown agent)".into());
                let context = agent
                    .map(|a| wt_context(&a.worktree_id))
                    .unwrap_or_default();
                let kind = agent.map(|a| a.kind.as_str()).unwrap_or("agents");
                (name, context, kind)
            }
            SessionRef::Terminal(id) => {
                let term = app.tree.terminals.iter().find(|t| &t.id == id);
                let name = term
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "(unknown terminal)".into());
                let context = term.map(|t| wt_context(&t.worktree_id)).unwrap_or_default();
                (name, context, "shells")
            }
        };
        let entry = kinds.entry(kind).or_default();
        entry.0 += 1;
        entry.1 += m.procs;
        entry.2 += m.rss_bytes;
        sessions_total += m.rss_bytes;
        rows.push(MetricsRow {
            name,
            context,
            pid: Some(m.pid),
            procs: m.procs,
            bytes: m.rss_bytes,
            sref: Some(m.session.clone()),
        });
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.bytes));
    append_warm_spares(&mut rows, spares);
    rows.push(MetricsRow {
        name: "kete daemon".into(),
        context: String::new(),
        pid: Some(snap.daemon_pid),
        procs: 1,
        bytes: snap.daemon_rss_bytes,
        sref: None,
    });
    rows.push(MetricsRow {
        name: "kete ui (this window)".into(),
        context: String::new(),
        pid: Some(std::process::id()),
        procs: 1,
        bytes: view.client_rss_bytes,
        sref: None,
    });
    (rows, kinds, sessions_total)
}

fn append_warm_spares(rows: &mut Vec<MetricsRow>, mut spares: Vec<MetricsRow>) {
    if spares.is_empty() {
        return;
    }
    spares.sort_by_key(|row| std::cmp::Reverse(row.bytes));
    let count = spares.len();
    rows.push(MetricsRow {
        name: format!("warm spares ({count})"),
        context: String::new(),
        pid: None,
        procs: spares.iter().map(|r| r.procs).sum(),
        bytes: spares.iter().map(|r| r.bytes).sum(),
        sref: None,
    });
    for (i, mut spare) in spares.into_iter().enumerate() {
        let branch = if i + 1 == count { "└ " } else { "├ " };
        spare.name = format!("{branch}{}", spare.name);
        rows.push(spare);
    }
}

fn draw_metrics_overlay(f: &mut Frame, app: &mut App, view: crate::app::MetricsView) {
    let th = app.chrome.theme;
    let (rows, kinds, sessions_total) = metrics_rows(app, &view);

    // The cursor follows the session it was on across refresh
    // re-sorts (sizes move rows around); nebula's own rows sit at
    // fixed positions, so the index fallback covers them.
    let prev = view.rows.get(view.selected).cloned().flatten();
    let selected = prev
        .and_then(|sref| rows.iter().position(|r| r.sref.as_ref() == Some(&sref)))
        .unwrap_or(view.selected)
        .min(rows.len().saturating_sub(1));

    let dim = Style::default().fg(th.dim);
    let header = Style::default().fg(th.muted).add_modifier(Modifier::BOLD);
    let mem_style = Style::default().fg(th.accent);
    let plural = |n: u32| if n == 1 { "" } else { "s" };

    let mut lines: Vec<Line> = Vec::new();
    let mut scroll = 0usize;
    let mut shown = 0usize;
    let mut rows_start = 0usize;
    if let Some(snap) = &view.snapshot {
        // Rollup: one line per agent kind, then nebula, then total.
        for (kind, (n, procs, bytes)) in &kinds {
            let unit = match *kind {
                "shells" => "terminal",
                "warm" => "spare",
                _ => "session",
            };
            let mut detail = format!("{n} {unit}{} · {procs} proc{}", plural(*n), plural(*procs));
            if *kind == "warm" {
                detail.push_str(" · pre-booted for new agents");
            }
            lines.push(Line::from(vec![
                Span::styled(format!(" {kind:<8} "), header),
                Span::styled(format!("{detail:<42}"), dim),
                Span::styled(format!("{:>9}", fmt_mem(*bytes)), mem_style),
            ]));
        }
        let nebula_bytes = snap.daemon_rss_bytes + view.client_rss_bytes;
        lines.push(Line::from(vec![
            Span::styled(" kete   ", header),
            Span::styled(format!("{:<42}", "daemon + this ui"), dim),
            Span::styled(format!("{:>9}", fmt_mem(nebula_bytes)), mem_style),
        ]));
        let total = sessions_total + nebula_bytes;
        let note = if snap.system_total_bytes > 0 {
            format!(
                "{:.1}% of {} installed",
                100.0 * total as f64 / snap.system_total_bytes as f64,
                fmt_mem(snap.system_total_bytes)
            )
        } else {
            String::new()
        };
        lines.push(Line::from(vec![
            Span::styled(" total    ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(format!("{note:<42}"), dim),
            Span::styled(
                format!("{:>9}", fmt_mem(total)),
                mem_style.add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                " {:<28} {:<15} {:>6} {:>5} {:>9}",
                "SESSION", "WHERE", "PID", "PROCS", "MEM"
            ),
            header,
        )));
        // Scrolled window over the rows; everything above stays put.
        let space = f.area().height.saturating_sub(lines.len() as u16 + 4) as usize;
        shown = rows.len().min(16).min(space.max(3));
        scroll = view.scroll.min(rows.len().saturating_sub(shown));
        // Keep the cursor inside the window.
        if selected < scroll {
            scroll = selected;
        } else if shown > 0 && selected >= scroll + shown {
            scroll = selected + 1 - shown;
        }
        rows_start = lines.len();
        for (i, row) in rows.iter().enumerate().skip(scroll).take(shown) {
            let name_style = if row.sref.is_none() {
                dim
            } else {
                Style::default()
            };
            let sel = |s: Style| {
                if i == selected {
                    s.bg(th.sel_bg).add_modifier(Modifier::BOLD)
                } else {
                    s
                }
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:<28} ", truncate(&row.name, 28)),
                    sel(name_style),
                ),
                Span::styled(format!("{:<15} ", truncate(&row.context, 15)), sel(dim)),
                Span::styled(
                    format!(
                        "{:>6} {:>5} ",
                        row.pid.map(|p| p.to_string()).unwrap_or_default(),
                        row.procs
                    ),
                    sel(dim),
                ),
                Span::styled(format!("{:>9}", fmt_mem(row.bytes)), sel(mem_style)),
            ]));
        }
        if rows.len() > shown {
            lines.push(Line::from(Span::styled(
                format!(" {}-{} of {}", scroll + 1, scroll + shown, rows.len()),
                dim,
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(" measuring…", dim)));
    }

    let height = (lines.len() as u16 + 2).min(f.area().height.saturating_sub(2));
    let area = centered_rect(f.area(), MEMORY_W, height);
    let inner = render_modal_frame(f, area, " Memory ", th);
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(Overlay::Metrics(v)) = &mut app.modals.overlay {
        v.area = area;
        v.scroll = scroll;
        v.selected = selected;
        v.rows = rows.into_iter().map(|r| r.sref).collect();
        v.list_area = Rect {
            x: inner.x,
            y: inner.y + rows_start as u16,
            width: inner.width,
            height: (shown as u16).min(inner.height.saturating_sub(rows_start as u16)),
        };
    }
}

fn draw_diff_overlay(f: &mut Frame, app: &mut App, view: crate::app::DiffView) {
    let th = app.chrome.theme;
    // The REVIEW UNIT docks beside the board, in the room the board left
    // it, its verb row along the foot; anywhere else the modal centers.
    let dock = app.launcher.review_dock;
    let docked =
        view.review.is_some() && app.launcher.launcher_kanban && dock.width > 0 && dock.height > 2;
    let area = if docked {
        Rect {
            height: dock.height - 1,
            ..dock
        }
    } else {
        centered_rect_pct(f.area(), SPLIT_MODAL_PCT.0, SPLIT_MODAL_PCT.1)
    };
    f.render_widget(Clear, area);
    if docked {
        let row = Rect {
            y: dock.bottom() - 1,
            height: 1,
            ..dock
        };
        let verbs = draw_review_verbs(f, app, &view, row);
        if let Some(Overlay::Diff(v)) = &mut app.modals.overlay {
            if let Some(r) = &mut v.review {
                r.verbs = verbs;
                r.dock = dock;
            }
        }
    }
    // Cap first, floor second: on a tiny screen the file list keeps
    // its minimum and SPLIT_PANE_LAYOUT_MIN squeezes the diff pane
    // instead.
    let files_w = view
        .files_width
        .min(area.width.saturating_sub(crate::app::MIN_DIFF_PANE_W))
        .max(crate::app::MIN_DIFF_FILES_W);
    let [files_a, diff_a] = Layout::horizontal([
        Constraint::Length(files_w),
        Constraint::Min(SPLIT_PANE_LAYOUT_MIN),
    ])
    .areas(area);

    let (against_area, session_area, files_a) = draw_diff_against(f, &view, files_a, th);
    let list_inner = draw_diff_file_list(f, &view, files_a, app.chrome.icons, th);

    // Right: the selected file's diff, scrolled — or, on a tree
    // directory's row, the list of what changed under it.
    let sel_path = if view.place == crate::app::Place::Graph {
        view.log
            .as_ref()
            .and_then(|log| log.selected_key())
            .unwrap_or_else(|| "GRAPH".into())
    } else if matches!(
        view.place,
        crate::app::Place::ChangesHeader | crate::app::Place::GraphHeader
    ) {
        "status".into()
    } else {
        match view.selected_dir() {
            Some(dir) => format!("{dir}/"),
            None => view.selected_path().unwrap_or("").to_string(),
        }
    };
    let sel_reviewed = view.reviewed.contains_key(&sel_path);
    let title = truncate(
        &format!(
            "{}: {}{}",
            view.branch,
            sel_path,
            if sel_reviewed { " ✓" } else { "" }
        ),
        (diff_a.width as usize).saturating_sub(4),
    );
    // The bottom rule says what the keys do here, as much of it as fits,
    // or — with a discard waiting on its answer — the question.
    let hint = if let Some(c) = view.confirm_discard {
        Span::styled(
            format!(
                " discard change {}? y/Enter: discard · any key: keep ",
                c + 1
            ),
            Style::default().fg(th.err).add_modifier(Modifier::BOLD),
        )
    } else {
        let room = (diff_a.width as usize).saturating_sub(12);
        let full = format!(
            " ^s {} · ^o context · ^n/^p change · ^y stage · ^x discard · ^r reviewed ",
            view.mode.label()
        );
        let short = format!(" ^s {} · ^n/^p · ^y/^x · ^r ", view.mode.label());
        let text = if full.chars().count() <= room {
            full
        } else {
            short
        };
        Span::styled(text, Style::default().fg(th.dim))
    };
    let mut block = panel_block(&title, true, th).title_bottom(Line::from(hint));
    let diff_inner = block.inner(diff_a);
    // The laid-out diff in the view's DIFF LAYOUT — side by side only when
    // the pane is wide enough for two numbered sides, inline otherwise —
    // or, for text with no hunk in it, the text as it is.
    let wide = view.mode != crate::diff_layout::DiffMode::SideBySide
        || diff_inner.width >= crate::app::MIN_SPLIT_W;
    let rows = view.laid.as_deref().map(|laid| match &laid.inline {
        Some(inline) if !wide => (laid, inline.as_slice(), false),
        _ => (
            laid,
            laid.rows.as_slice(),
            view.mode == crate::diff_layout::DiffMode::SideBySide,
        ),
    });
    let total = rows.map_or(view.diff_line_count, |(_, r, _)| r.len());
    let max_scroll = (total as u16).saturating_sub(diff_inner.height.max(1));
    let scroll = view.scroll.min(max_scroll);
    if max_scroll > 0 {
        block = block.title_bottom(
            Line::from(Span::styled(
                format!(" {}/{} ", scroll + 1, total),
                Style::default().fg(th.dim),
            ))
            .right_aligned(),
        );
    }
    f.render_widget(block, diff_a);
    match rows {
        Some((laid, rows, two_sides)) => {
            let look = DiffLook {
                laid,
                mode: view.mode,
                cursor: view.change_cursor,
                staged: &|c| view.change_staged(c),
                th,
            };
            draw_laid_diff(f, &look, rows, scroll, diff_inner, two_sides);
        }
        None => {
            // Only the rows in view are styled: a diff runs to 20 000
            // lines, and building a `Line` for each of them on every frame
            // was most of what scrolling a large one cost.
            let lines: Vec<Line> = view
                .diff
                .lines()
                .skip(scroll as usize)
                .take(diff_inner.height as usize)
                .map(|l| Line::from(Span::styled(l.to_string(), diff_line_style(l, th))))
                .collect();
            f.render_widget(Paragraph::new(lines), diff_inner);
        }
    }

    // Write-back (draw works on a clone): page size for key paging,
    // scroll re-clamped so resizes never strand the view.
    if let Some(Overlay::Diff(v)) = &mut app.modals.overlay {
        v.view_height = diff_inner.height;
        v.scroll = scroll;
        v.list_area = list_inner;
        v.area = area;
        v.files_width = files_w;
        v.wide_shown = wide;
        v.against_area = against_area;
        v.attribution.area = session_area;
    }
}

/// The REVIEW UNIT's session filter under the against toggle, when more
/// than one session has worked in the worktree: `all in worktree` or
/// `this session`, the one shown lit, as a toggle (`Ctrl+l`, or a
/// click). Returns its rect (empty when not drawn) and what is left.
fn draw_diff_session(
    f: &mut Frame,
    view: &crate::app::DiffView,
    col: Rect,
    th: Theme,
) -> (Rect, Rect) {
    let offered = view
        .attribution
        .session
        .as_ref()
        .is_some_and(|s| s.offered());
    if !offered || col.height < 8 {
        return (Rect::default(), col);
    }
    let [row, rest] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(col);
    let lit = Style::default()
        .fg(th.accent)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let off = Style::default().fg(th.dim);
    let (all, mine) = if view.attribution.listed {
        (off, lit)
    } else {
        (lit, off)
    };
    let width = row.width as usize;
    // The long words give way to short ones, and only then the key's hint.
    const HINT: &str = " ^l";
    let fits = |a: &str, b: &str| a.chars().count() + b.chars().count() + HINT.len() <= width;
    let words = [
        (" all in worktree ", " this session "),
        (" all ", " this session "),
        (" all ", " session "),
    ];
    let (a, b) = words
        .iter()
        .copied()
        .find(|(a, b)| fits(a, b))
        .unwrap_or(words[2]);
    let mut spans = vec![Span::styled(a, all), Span::styled(b, mine)];
    if fits(a, b) {
        spans.push(Span::styled(HINT, off));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row);
    (row, rest)
}

/// The most rows the COMMITS strip takes above the files.
const COMMIT_STRIP_ROWS: u16 = 5;

/// The head of the DIFF VIEWER's file column: what the files are read
/// against — `vs main (merge-base abc1234)` or `vs HEAD` — as a toggle
/// (`Ctrl+g`, or a click), and under it, with a base, the COMMITS made
/// since it, read-only; in a REVIEW UNIT with more than one session's
/// snapshots, the session chip between the two (`draw_diff_session`).
/// Returns the toggle's rect (empty without a base, so a click misses
/// it), the chip's (empty when not drawn) and what is left of `col` for
/// the file list.
fn draw_diff_against(
    f: &mut Frame,
    view: &crate::app::DiffView,
    col: Rect,
    th: Theme,
) -> (Rect, Rect, Rect) {
    // A pull request's view is read against its own base, and says so in
    // its title already.
    if view.prefetched.is_some() || col.height < 8 {
        return (Rect::default(), Rect::default(), col);
    }
    let [head, rest] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(col);
    let (session, rest) = draw_diff_session(f, view, rest, th);
    let label = view.against_label();
    let line = match &view.base {
        Some(base) => {
            let other = match view.against {
                nebula_core::review_base::Against::Base => "HEAD".to_string(),
                nebula_core::review_base::Against::Head => base.branch.clone(),
            };
            // The key's word gives way first, then the label clips.
            let width = head.width as usize;
            let label = truncate(&label, width.saturating_sub(5));
            let used = label.chars().count() + 2;
            let full = format!(" ^g: vs {other}");
            let hint = if used + full.chars().count() <= width {
                full
            } else {
                " ^g".to_string()
            };
            Line::from(vec![
                Span::styled(
                    format!(" {label} "),
                    Style::default()
                        .fg(th.accent)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                ),
                Span::styled(hint, Style::default().fg(th.dim)),
            ])
        }
        None => Line::from(Span::styled(
            format!(" {label} · no base branch"),
            Style::default().fg(th.dim),
        )),
    };
    f.render_widget(Paragraph::new(line), head);
    let toggle = if view.base.is_some() {
        head
    } else {
        Rect::default()
    };
    if view.base.is_none() || view.commits.is_empty() {
        return (toggle, session, rest);
    }
    // Room for the strip and still a usable file list under it.
    let rows = (view.commits.len() as u16)
        .min(COMMIT_STRIP_ROWS)
        .min(rest.height.saturating_sub(8) / 2);
    if rows == 0 {
        return (toggle, session, rest);
    }
    let [strip, files] =
        Layout::vertical([Constraint::Length(rows + 2), Constraint::Min(0)]).areas(rest);
    let more = view.commits.len() > rows as usize;
    let title = format!(
        "Commits ({}{})",
        view.commits.len(),
        if view.commits.len() >= nebula_core::review_base::MAX_COMMITS {
            "+"
        } else {
            ""
        }
    );
    let mut block = panel_block(&title, false, th);
    if more {
        block = block.title_bottom(Line::from(Span::styled(
            format!(" +{} more ", view.commits.len() - rows as usize),
            Style::default().fg(th.dim),
        )));
    }
    let inner = block.inner(strip);
    f.render_widget(block, strip);
    let width = inner.width as usize;
    let lines: Vec<Line> = view
        .commits
        .iter()
        .take(rows as usize)
        .map(|c| {
            let subject = truncate(&c.subject, width.saturating_sub(c.short.len() + 1));
            Line::from(vec![
                Span::styled(format!("{} ", c.short), Style::default().fg(th.dim)),
                Span::raw(subject),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
    (toggle, session, files)
}

/// A unified diff line's color: added green, removed red, hunk headers in
/// the accent, file headers dimmed.
fn diff_line_style(line: &str, th: Theme) -> Style {
    match classify_diff_line(line) {
        DiffLineKind::Add => Style::default().fg(th.ok),
        DiffLineKind::Remove => Style::default().fg(th.err),
        DiffLineKind::Hunk => Style::default().fg(th.accent),
        DiffLineKind::Header => Style::default().fg(th.dim),
        DiffLineKind::Context => Style::default(),
    }
}

/// What every row of a laid-out diff is drawn with: the document, the
/// CHANGE the cursor is on, whether a change is staged, the theme.
struct DiffLook<'a> {
    laid: &'a crate::app::LaidDiff,
    mode: crate::diff_layout::DiffMode,
    cursor: Option<usize>,
    staged: &'a dyn Fn(usize) -> Option<bool>,
    th: Theme,
}

/// The gutter's mark for a row of CHANGE `change`: an accent bar on the
/// change the cursor is on, a `✓` on a staged one, blank otherwise.
fn change_mark(look: &DiffLook, change: Option<usize>) -> Span<'static> {
    match change {
        Some(c) if look.cursor == Some(c) => Span::styled("▌", Style::default().fg(look.th.accent)),
        Some(c) if (look.staged)(c) == Some(true) => {
            Span::styled("✓", Style::default().fg(look.th.ok))
        }
        _ => Span::raw(" "),
    }
}

/// A line number right-aligned in `w` columns, or blank.
fn line_no(n: Option<u32>, w: usize) -> String {
    match n {
        Some(n) => format!("{n:>w$}"),
        None => " ".repeat(w),
    }
}

/// Line `i`'s text as spans: its syntax colors, over its side's wash
/// (none for an unchanged line), with the words its partner line does not
/// share lit stronger — `width` columns of it, the wash run to the edge.
fn line_spans(look: &DiffLook, i: usize, width: usize) -> Vec<Span<'static>> {
    use crate::diff_layout::Kind;
    let doc = &look.laid.doc;
    let th = look.th;
    let line = &doc.lines[i];
    let (wash, word) = match line.kind {
        Kind::Add => (Some(th.diff_add_bg), th.diff_add_word),
        Kind::Remove => (Some(th.diff_del_bg), th.diff_del_word),
        Kind::Context => (None, th.sel_bg),
    };
    let ranges = match (line.kind, line.partner) {
        (Kind::Add, Some(p)) => crate::diff_layout::word_ranges(&doc.lines[p].text, &line.text).1,
        (Kind::Remove, Some(p)) => {
            crate::diff_layout::word_ranges(&line.text, &doc.lines[p].text).0
        }
        _ => Vec::new(),
    };
    let mut spans = Vec::new();
    let mut at = 0usize;
    for (kind, text) in &line.tokens {
        let base = crate::ui::token_style(*kind, th);
        let base = match wash {
            Some(bg) => base.bg(bg),
            None => base,
        };
        // Split the run where a changed range starts or ends.
        let mut run = String::new();
        let mut lit = false;
        for ch in text.chars() {
            if at >= width {
                break;
            }
            let now = ranges.iter().any(|&(a, b)| (a..b).contains(&at));
            if now != lit && !run.is_empty() {
                let style = if lit { base.bg(word) } else { base };
                spans.push(Span::styled(std::mem::take(&mut run), style));
            }
            lit = now;
            run.push(ch);
            at += 1;
        }
        if !run.is_empty() {
            let style = if lit { base.bg(word) } else { base };
            spans.push(Span::styled(run, style));
        }
    }
    if let Some(bg) = wash {
        if at < width {
            spans.push(Span::styled(
                " ".repeat(width - at),
                Style::default().bg(bg),
            ));
        }
    }
    spans
}

/// The sign column of a line: `+`, `-` or blank, over its wash.
fn sign_span(look: &DiffLook, i: usize) -> Span<'static> {
    use crate::diff_layout::Kind;
    let th = look.th;
    match look.laid.doc.lines[i].kind {
        Kind::Add => Span::styled("+", Style::default().fg(th.ok).bg(th.diff_add_bg)),
        Kind::Remove => Span::styled("-", Style::default().fg(th.err).bg(th.diff_del_bg)),
        Kind::Context => Span::raw(" "),
    }
}

/// A hunk's row: what the diff left out above it, then the section git
/// named after its `@@` (the function the hunk is in), in the accent.
fn hunk_spans(look: &DiffLook, h: usize) -> Vec<Span<'static>> {
    let hunk = &look.laid.doc.hunks[h];
    let th = look.th;
    let dim = Style::default().fg(th.dim);
    let mut spans = Vec::new();
    if hunk.hidden > 0 {
        spans.push(Span::styled(
            format!(
                "⋯ {} unchanged line{}",
                hunk.hidden,
                if hunk.hidden == 1 { "" } else { "s" }
            ),
            dim,
        ));
    } else {
        spans.push(Span::styled("⋯", dim));
    }
    let section = hunk
        .header
        .splitn(3, "@@")
        .nth(2)
        .map(str::trim)
        .unwrap_or("");
    let ranges = format!(
        "  @@ -{},{} +{},{} @@",
        hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len
    );
    spans.push(Span::styled(ranges, dim));
    if !section.is_empty() {
        spans.push(Span::styled(
            format!(" {section}"),
            Style::default().fg(th.accent),
        ));
    }
    spans
}

/// A laid-out diff (`diff_layout`), the rows in view only. Side by side,
/// each half has its own line numbers; inline, an old and a new gutter;
/// in the replacement layout, the new numbers alone. Every row starts on
/// its CHANGE's mark ([`change_mark`]).
fn draw_laid_diff(
    f: &mut Frame,
    look: &DiffLook,
    rows: &[crate::diff_layout::LaidRow],
    scroll: u16,
    area: Rect,
    two_sides: bool,
) {
    use crate::diff_layout::Row;
    let th = look.th;
    let doc = &look.laid.doc;
    let widest = doc
        .lines
        .iter()
        .map(|l| l.old.unwrap_or(0).max(l.new.unwrap_or(0)))
        .max()
        .unwrap_or(0);
    let w = widest.to_string().len();
    let dim = Style::default().fg(th.dim);
    let visible = rows.iter().skip(scroll as usize).take(area.height as usize);
    if two_sides {
        let half = area.width.saturating_sub(1) / 2;
        let [left_a, sep_a, right_a] = Layout::horizontal([
            Constraint::Length(half),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);
        let side = |i: Option<usize>, old: bool, cols: u16| -> Line<'static> {
            let Some(i) = i else {
                return Line::default();
            };
            let line = &doc.lines[i];
            let n = if old { line.old } else { line.new };
            let text_w = (cols as usize).saturating_sub(w + 3);
            let mut spans = vec![
                Span::styled(format!("{} ", line_no(n, w)), dim),
                sign_span(look, i),
            ];
            spans.extend(line_spans(look, i, text_w));
            Line::from(spans)
        };
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for r in visible {
            let mark = change_mark(look, r.change);
            match &r.row {
                Row::Pair(old, new) => {
                    let mut l = side(*old, true, left_a.width.saturating_sub(1));
                    l.spans.insert(0, mark);
                    left.push(l);
                    right.push(side(*new, false, right_a.width));
                }
                Row::Hunk(h) => {
                    let mut spans = vec![mark];
                    spans.extend(hunk_spans(look, *h));
                    left.push(Line::from(spans));
                    right.push(Line::default());
                }
                Row::Note(text, kind) => {
                    let style = if *kind == DiffLineKind::Hunk {
                        Style::default().fg(th.accent)
                    } else {
                        dim
                    };
                    left.push(Line::from(vec![mark, Span::styled(text.clone(), style)]));
                    right.push(Line::default());
                }
                Row::Line(i) => {
                    left.push(Line::from(vec![mark]));
                    right.push(side(Some(*i), false, right_a.width));
                }
                Row::Removed { .. } => {
                    left.push(Line::default());
                    right.push(Line::default());
                }
            }
        }
        let seam = vec![Line::from(Span::styled("│", dim)); area.height as usize];
        f.render_widget(Paragraph::new(left), left_a);
        f.render_widget(Paragraph::new(seam), sep_a);
        f.render_widget(Paragraph::new(right), right_a);
        return;
    }
    // Inline and replacement: one column. Inline numbers both sides; the
    // replacement layout shows only the new file, so only its numbers.
    let inline = look.mode != crate::diff_layout::DiffMode::Replacement;
    let gutter_w = if inline { 2 * w + 2 } else { w + 1 };
    let text_w = (area.width as usize).saturating_sub(1 + gutter_w + 1);
    let mut lines = Vec::new();
    for r in visible {
        let mut spans = vec![change_mark(look, r.change)];
        match &r.row {
            Row::Line(i) => {
                let line = &doc.lines[*i];
                let gutter = if inline {
                    format!("{} {} ", line_no(line.old, w), line_no(line.new, w))
                } else {
                    format!("{} ", line_no(line.new, w))
                };
                spans.push(Span::styled(gutter, dim));
                spans.push(sign_span(look, *i));
                spans.extend(line_spans(look, *i, text_w));
            }
            Row::Removed { first, count } => {
                let at = doc.lines[*first].old.unwrap_or(0);
                spans.push(Span::styled(" ".repeat(gutter_w), dim));
                spans.push(Span::styled(
                    format!(
                        "− {count} line{} removed (old {at}{})",
                        if *count == 1 { "" } else { "s" },
                        if *count > 1 {
                            format!("–{}", at + count - 1)
                        } else {
                            String::new()
                        }
                    ),
                    Style::default().fg(th.err).bg(th.diff_del_bg),
                ));
            }
            Row::Hunk(h) => spans.extend(hunk_spans(look, *h)),
            Row::Note(text, _) => spans.push(Span::styled(text.clone(), dim)),
            Row::Pair(..) => {}
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// A section header's fold marker and name, bold.
fn header_spans(open: bool, name: &str, th: Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            if open { "▾ " } else { "▸ " },
            Style::default().fg(th.accent),
        ),
        Span::styled(
            name.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]
}

fn changes_header_spans(view: &crate::app::DiffView, th: Theme) -> Vec<Span<'static>> {
    let name = if view.prefetched.is_some() {
        "FILES"
    } else {
        "CHANGES"
    };
    let mut spans = header_spans(view.changes_open, name, th);
    let count = if view.listing.is_some() && view.files.is_empty() {
        " (…)".to_string()
    } else if view.filter.is_empty() {
        format!(" ({})", view.files.len())
    } else {
        format!(" ({}/{})", view.matches.len(), view.files.len())
    };
    spans.push(Span::styled(count, Style::default().fg(th.dim)));
    if !view.reviewed.is_empty() {
        spans.push(Span::styled(
            format!(" · {}✓", view.reviewed.len()),
            Style::default().fg(th.dim),
        ));
    }
    spans
}

fn graph_header_spans(view: &crate::app::DiffView, th: Theme) -> Vec<Span<'static>> {
    let mut spans = header_spans(view.graph_open, "GRAPH", th);
    let Some(log) = &view.log else {
        return spans;
    };
    let commits = log
        .rows
        .iter()
        .filter(|r| matches!(r.entry, crate::git_log::Entry::Commit(_)));
    let count = if log.reading.is_some() && log.commits.is_empty() {
        " (…)".to_string()
    } else if view.filter.is_empty() {
        format!(" ({})", log.commits.len())
    } else {
        format!(" ({}/{})", commits.count(), log.commits.len())
    };
    spans.push(Span::styled(count, Style::default().fg(th.dim)));
    let (ahead, behind) = log.sides();
    if ahead > 0 {
        spans.push(Span::styled(
            format!("  ↑{ahead}"),
            Style::default().fg(th.ok),
        ));
    }
    if behind > 0 {
        spans.push(Span::styled(
            format!("  ↓{behind}"),
            Style::default().fg(th.warn),
        ));
    }
    spans
}

fn change_gutter(
    file: Option<&crate::git_diff::DiffFile>,
    reviewed: bool,
    th: Theme,
) -> Vec<Span<'static>> {
    let status = match file {
        Some(file) => Span::styled(
            format!("{} ", file.status_str()),
            Style::default().fg(match (file.xy[0], file.xy[1]) {
                ('?', '?') | ('A', _) => th.ok,
                ('D', _) | (_, 'D') => th.err,
                ('R', _) | ('C', _) => th.accent,
                _ => th.warn,
            }),
        ),
        None => Span::raw("   "),
    };
    let mark = if reviewed {
        Span::styled("✓ ", Style::default().fg(th.ok))
    } else {
        Span::raw("  ")
    };
    vec![status, mark]
}

fn change_row_spans(
    view: &crate::app::DiffView,
    i: usize,
    done: Option<&[bool]>,
    width: u16,
    icons: crate::icons::IconStyle,
    th: Theme,
) -> Vec<Span<'static>> {
    let Some(tree) = &view.tree else {
        let Some(m) = view.matches.get(i) else {
            return Vec::new();
        };
        let file = &view.files[m.file];
        let icon = crate::icons::file_span(&file.path, icons, th.light);
        let budget = (width as usize).saturating_sub(5 + icon.as_ref().map_or(0, |s| s.width()));
        let mut spans = change_gutter(Some(file), view.reviewed.contains_key(&file.path), th);
        spans.extend(icon);
        let shown = truncate(&file.path, budget);
        let used = shown.chars().count();
        spans.extend(fuzzy_highlight_spans(&shown, &m.positions, th));
        if let Some(orig) = &file.orig_path {
            let rest = budget.saturating_sub(used);
            if rest > 3 {
                spans.push(Span::styled(
                    truncate(&format!(" ← {orig}"), rest),
                    Style::default().fg(th.dim),
                ));
            }
        }
        return spans;
    };
    let Some(r) = tree.rows.get(i) else {
        return Vec::new();
    };
    let node = &tree.nodes[r.node];
    let file = tree.file_of[r.node].map(|f| &view.files[f]);
    let indent = "  ".repeat(node.depth);
    let marker = if !node.is_dir {
        "  "
    } else if tree.is_open(r.node, !view.filter.is_empty()) {
        "▾ "
    } else {
        "▸ "
    };
    let icon = if node.is_dir {
        let open = tree.is_open(r.node, !view.filter.is_empty());
        crate::icons::folder_span(&node.name, open, icons, th.light)
    } else {
        crate::icons::file_span(&node.name, icons, th.light)
    };
    let icon_w = icon.as_ref().map_or(0, |s| s.width());
    let budget = (width as usize).saturating_sub(5 + indent.chars().count() + 2 + icon_w);
    let shown = truncate(&node.name, budget);
    let reviewed = done.is_some_and(|d| d[r.node]);
    let mut spans = change_gutter(file, reviewed, th);
    spans.push(Span::raw(indent));
    spans.push(Span::styled(marker, Style::default().fg(th.accent)));
    spans.extend(icon);
    if node.is_dir {
        spans.push(Span::styled(shown, Style::default().fg(th.accent)));
    } else {
        let positions = visible_positions(&r.positions, &shown, &node.name);
        spans.extend(fuzzy_highlight_spans(&shown, positions, th));
    }
    spans
}

fn graph_spans(graph: &str, under: bool, th: Theme) -> Vec<Span<'static>> {
    let lanes = [th.accent, th.ok, th.warn, th.special, th.merged, th.err];
    graph
        .chars()
        .enumerate()
        .map(|(col, ch)| {
            let glyph = match ch {
                '*' if under => '│',
                '*' => '●',
                '|' => '│',
                '/' => '╱',
                '\\' => '╲',
                '-' | '_' => '─',
                other => other,
            };
            Span::styled(
                glyph.to_string(),
                Style::default().fg(lanes[col / 2 % lanes.len()]),
            )
        })
        .collect()
}

fn log_row_spans(
    log: &crate::git_log::GitLog,
    i: usize,
    now: i64,
    th: Theme,
) -> Vec<Span<'static>> {
    use crate::git_log::{Entry, RefKind, Side};
    let Some(r) = log.rows.get(i) else {
        return Vec::new();
    };
    let dim = Style::default().fg(th.dim);
    let graph = r.line.map_or("", |n| log.lines[n].0.as_str());
    let under = matches!(r.entry, Entry::File(..) | Entry::Note(_));
    let mut spans = graph_spans(graph, under, th);
    if !graph.is_empty() {
        spans.push(Span::raw(" "));
    }
    match r.entry {
        Entry::Graph => {}
        Entry::Note(c) => spans.push(Span::styled(format!("   {}", log.note(c)), dim)),
        Entry::File(c, f) => {
            if let Some(file) = log.file_at(c, f) {
                spans.push(Span::raw("  "));
                spans.extend(change_gutter(Some(file), false, th).into_iter().take(1));
                spans.push(Span::raw(file.path.clone()));
                if let Some(orig) = &file.orig_path {
                    spans.push(Span::styled(format!(" ← {orig}"), dim));
                }
            }
        }
        Entry::Commit(c) => {
            let c = &log.commits[c];
            match c.side {
                Side::Ahead => spans.push(Span::styled("↑ ", Style::default().fg(th.ok))),
                Side::Behind => spans.push(Span::styled("↓ ", Style::default().fg(th.warn))),
                Side::Shared => {}
            }
            if r.positions.is_empty() {
                spans.push(Span::styled(c.short.clone(), Style::default().fg(th.muted)));
                if !c.refs.is_empty() {
                    spans.push(Span::raw(" ("));
                    for (n, rf) in c.refs.iter().enumerate() {
                        if n > 0 {
                            spans.push(Span::raw(", "));
                        }
                        let style = Style::default().fg(match rf.kind {
                            RefKind::Head | RefKind::Local => th.ok,
                            RefKind::Remote => th.accent,
                            RefKind::Tag => th.warn,
                        });
                        let style = if rf.kind == RefKind::Head {
                            style.add_modifier(Modifier::BOLD)
                        } else {
                            style
                        };
                        spans.push(Span::styled(rf.label(), style));
                    }
                    spans.push(Span::raw(")"));
                }
                spans.push(Span::raw(format!(" {}", c.subject)));
            } else {
                spans.extend(fuzzy_highlight_spans(&c.haystack(), &r.positions, th));
            }
            let ago = crate::hosts::ago_label(now - c.time * 1000);
            spans.push(Span::styled(format!("  {} · {ago}", c.author), dim));
        }
    }
    spans
}

fn draw_diff_file_list(
    f: &mut Frame,
    view: &crate::app::DiffView,
    files_a: Rect,
    icons: crate::icons::IconStyle,
    th: Theme,
) -> Rect {
    // Left: changed-file list — flat paths, or the directory tree
    // (`Ctrl+t`); a stateless follow-window keeps the selected row
    // visible.
    let mut files_title = if view.log.is_some() {
        "Source control".to_string()
    } else if view.listing.is_some() && view.files.is_empty() {
        "Files (…)".to_string()
    } else if view.filter.is_empty() {
        format!("Files ({})", view.files.len())
    } else {
        format!("Files ({}/{})", view.matches.len(), view.files.len())
    };
    if !view.reviewed.is_empty() {
        files_title.push_str(&format!(" · {}✓", view.reviewed.len()));
    }
    // The hint names the list `Ctrl+t` leads to, not the one up.
    let block = panel_block(&files_title, true, th).title_bottom(Line::from(Span::styled(
        if view.tree.is_some() {
            " ^t: flat list "
        } else {
            " ^t: tree "
        },
        Style::default().fg(th.dim),
    )));
    let files_inner = block.inner(files_a);
    f.render_widget(block, files_a);

    // First row: the always-on fuzzy filter input.
    if let Some(filter_area) = row_rect(files_inner, 0) {
        let line = search_line(&view.filter, "type to filter…", filter_area, th);
        f.render_widget(Paragraph::new(line), filter_area);
    }
    let list_inner = below_first_row(files_inner);

    if view.log.is_some() {
        let done = view
            .tree
            .as_ref()
            .map(|tree| tree.reviewed_nodes(&view.files, &view.reviewed));
        let start = crate::app::window_start(view.side_cursor(), list_inner.height as usize);
        let now = crate::app::now_ms() as i64;
        for row in 0..list_inner.height as usize {
            let index = start + row;
            let Some((place, inner)) = view.side_row(index) else {
                break;
            };
            let Some(row_area) = row_rect(list_inner, row) else {
                break;
            };
            let spans = match place {
                crate::app::Place::ChangesHeader => changes_header_spans(view, th),
                crate::app::Place::Changes => {
                    change_row_spans(view, inner, done.as_deref(), list_inner.width, icons, th)
                }
                crate::app::Place::GraphHeader => graph_header_spans(view, th),
                crate::app::Place::Graph => view
                    .log
                    .as_ref()
                    .map(|log| log_row_spans(log, inner, now, th))
                    .unwrap_or_default(),
            };
            render_row(f, row_area, spans, index == view.side_cursor(), true, th);
        }
    } else if view.listing.is_some() && view.files.is_empty() {
        empty_list_row(f, list_inner, "reading changes…", th);
    } else {
        if view.row_count() == 0 {
            empty_list_row(f, list_inner, NO_MATCHES, th);
        }
        let start = view.window_start(list_inner.height as usize);
        match &view.tree {
            None => {
                for (row, (i, _)) in view.matches.iter().enumerate().skip(start).enumerate() {
                    let Some(row_area) = row_rect(list_inner, row) else {
                        break;
                    };
                    let spans = change_row_spans(view, i, None, list_inner.width, icons, th);
                    render_row(f, row_area, spans, i == view.selected, true, th);
                }
            }
            Some(tree) => {
                let done = tree.reviewed_nodes(&view.files, &view.reviewed);
                for (row, (i, _)) in tree.rows.iter().enumerate().skip(start).enumerate() {
                    let Some(row_area) = row_rect(list_inner, row) else {
                        break;
                    };
                    let spans = change_row_spans(view, i, Some(&done), list_inner.width, icons, th);
                    render_row(f, row_area, spans, i == tree.selected, true, th);
                }
            }
        }
    }

    list_inner
}

fn draw_palette_overlay(f: &mut Frame, app: &mut App, palette: crate::palette::Palette) {
    let th = app.chrome.theme;
    let area = centered_rect(f.area(), PALETTE_SIZE.0, PALETTE_SIZE.1);
    let name = if palette.commands {
        "Commands"
    } else {
        "Jump to"
    };
    let title = if palette.list.query.is_empty() {
        format!(" {name} ")
    } else {
        format!(" {name} ({}/{}) ", palette.hits(), palette.list.items.len())
    };
    let inner = render_modal_frame(f, area, title, th);

    // First row: the always-on fuzzy query input.
    if let Some(query_area) = row_rect(inner, 0) {
        let line = search_line(&palette.list.query, "type to search…", query_area, th);
        f.render_widget(Paragraph::new(line), query_area);
    }
    let list_inner = below_first_row(inner);

    if palette.list.matches.is_empty() {
        empty_list_row(f, list_inner, NO_MATCHES, th);
    }
    let start = palette.list.window_start(list_inner.height as usize);
    for (row, (i, m)) in palette
        .list
        .matches
        .iter()
        .enumerate()
        .skip(start)
        .enumerate()
    {
        let Some(row_area) = row_rect(list_inner, row) else {
            break;
        };
        let item = &palette.list.items[m.item];
        // Kind lives in the glyph's shape; its color — and the
        // hollow variant standing in for the panels' `○` — come
        // from the same status the row carries in its panel, so a
        // running session reads as running here too. The row draws
        // the project it lives in dim, then its own name — a
        // project row in bold, a dim "23m ago" pinned right — so
        // the cyan-bold match highlight is the loudest thing in the
        // list, and a title sweeps exactly like its panel row.
        let (solid, hollow) = match &item.target {
            PaletteTarget::Project(_) => ("▪ ", "▫ "),
            PaletteTarget::Worktree(_) => ("▸ ", "▹ "),
            PaletteTarget::Session(_) => ("● ", "○ "),
            // The arrow its Worktrees-panel row wears (`pr_row`),
            // since that row is where picking it lands.
            PaletteTarget::PullRequest { .. } => ("↗ ", "↗ "),
            // The COMMAND PALETTE's actions and jumps.
            PaletteTarget::Action(_) | PaletteTarget::Attention(_) => ("› ", "› "),
        };
        let status = item.status;
        // A pull request carries no status; its colors are its
        // standing's, the look its Worktrees-panel row wears — the
        // accent for one ready for review, the dim end to end for
        // a draft, red for one GitHub says cannot merge — and a
        // trailing badge spells that state out in full (`draft`,
        // `ready for review`, or the trouble: `merge conflicts`,
        // `checks failing`), the sidebar's words at this modal's
        // width, so the rows are told apart before one is picked,
        // by the word and not only by the color.
        let pr = item
            .standing
            .map(|standing| (standing, crate::pr_row::look(standing, item.trouble, th)));
        let (glyph, glyph_color) = if let Some((_, look)) = pr {
            (solid, look.glyph)
        } else {
            match status {
                Some(AgentStatus::Running) => (solid, th.warn),
                Some(AgentStatus::Finished) if item.unseen => (solid, th.done),
                Some(AgentStatus::Finished) => (solid, th.ok),
                Some(AgentStatus::NeedsFeedback) => (solid, th.err),
                Some(AgentStatus::Terminated) => (solid, th.special),
                Some(AgentStatus::Fresh) => (solid, th.dim),
                Some(AgentStatus::Disconnected) | None => (hollow, th.dim),
            }
        };
        let badge = pr.map(|(standing, look)| {
            let word = item.trouble.map_or(standing.label(), |t| t.label());
            (format!(" {word}"), look.badge)
        });
        // The label is the row's own name, with the project it
        // lives in drawn dim in front of it — `demo/fix-login`, one
        // line, no header above it. The rest of the searched path
        // (a session's branch) still narrows the list; it is simply
        // not drawn.
        let label: String = item.text.chars().skip(item.label_at).collect();
        let label_positions: Vec<usize> = m
            .positions
            .iter()
            .filter_map(|p| p.checked_sub(item.label_at))
            .collect();
        let (crumb, crumb_hits) = match item.crumb {
            Some((at, end)) => (
                format!(
                    "{}/",
                    item.text.chars().take(end).skip(at).collect::<String>()
                ),
                m.positions
                    .iter()
                    .filter(|p| (at..end).contains(p))
                    .map(|p| p - at)
                    .collect(),
            ),
            None => (String::new(), Vec::new()),
        };
        // Pinned right, dim: when the row last ran — its panel
        // row's "23m ago".
        // An action's row ends in its key instead, where it has one.
        let tail = if let PaletteTarget::Action(action) = &item.target {
            app.chrome
                .keymap
                .shown_first(*action)
                .map(|c| c.display())
                .unwrap_or_default()
        } else if item.stamped > 0 {
            crate::hosts::ago_label(crate::app::now_ms() - item.stamped)
        } else {
            String::new()
        };
        let tail_w = tail.chars().count();
        // The badge is billed before the text, as `pr_row::spans`
        // does, so a long title shortens and the state never clips;
        // the tail too, with a two-column gap before it. The width
        // leaves the selection marker's column and a right margin.
        let badge_len = badge.as_ref().map_or(0, |(b, _)| b.chars().count());
        let width = (list_inner.width as usize).saturating_sub(2);
        // The crumb never eats the row: a long project name gets a
        // third of the width, the row's own name keeps the rest.
        let crumb_shown = truncate(&crumb, width / 3);
        let crumb_hits = visible_positions(&crumb_hits, &crumb_shown, &crumb);
        let lead = 2 + crumb_shown.chars().count();
        let budget = width
            .saturating_sub(lead + badge_len)
            .saturating_sub(if tail_w > 0 { tail_w + 2 } else { 0 });
        let shown = truncate(&label, budget);
        let positions = visible_positions(&label_positions, &shown, &label);
        let quiet = item.trouble.is_none()
            && matches!(item.standing, Some(crate::pull_request::Standing::Draft));
        let mut text = label_highlight_spans(
            &shown,
            positions,
            quiet,
            // No ONE-SHOT SWEEP in a list the user just summoned:
            // it is for the change nobody was looking at.
            sweep_ramp(status, false, th, app.chrome.animations),
            app.sweep_phase(),
            // A pull request in trouble paints its title in its
            // row's red — the end-to-end red its sidebar row wears.
            pr.filter(|_| item.trouble.is_some())
                .map_or(th.text, |(_, look)| look.label),
            th,
        );
        if matches!(item.target, PaletteTarget::Project(_)) {
            for s in &mut text {
                s.style = s.style.add_modifier(Modifier::BOLD);
            }
        }
        let mut spans = vec![Span::styled(glyph, Style::default().fg(glyph_color))];
        if !crumb_shown.is_empty() {
            // Dim end to end, bar the chars the query hit: the crumb
            // places the row, the name is what you are reading for.
            spans.extend(label_highlight_spans(
                &crumb_shown,
                crumb_hits,
                true,
                None,
                0,
                th.dim,
                th,
            ));
        }
        spans.extend(text);
        if let Some((badge, color)) = badge {
            spans.push(Span::styled(badge, Style::default().fg(color)));
        }
        let used = lead + shown.chars().count() + badge_len;
        if tail_w > 0 && used + tail_w < width {
            spans.push(Span::raw(" ".repeat(width - used - tail_w)));
            spans.push(Span::styled(tail, Style::default().fg(th.dim)));
        }
        render_row(f, row_area, spans, i == palette.list.cursor, true, th);
    }

    // Write-back (draw works on a clone): rects for mouse
    // hit-testing.
    if let Some(Overlay::Palette(p)) = &mut app.modals.overlay {
        p.area = area;
        p.list.list_area = list_inner;
        p.list.sync_scroll(list_inner.height as usize);
    }
}

fn draw_files_overlay(f: &mut Frame, app: &mut App, finder: crate::app::FileFinder) {
    let th = app.chrome.theme;
    let area = centered_rect(f.area(), FILES_SIZE.0, FILES_SIZE.1);
    // No count to show until the listing lands: `(0/0)` reads as
    // "no files", which is not what is known yet.
    let title = if finder.listing.is_some() {
        format!(" Find file: {} (listing…) ", finder.branch)
    } else if finder.list.query.is_empty() {
        format!(
            " Find file: {} ({}) ",
            finder.branch,
            finder.list.items.len()
        )
    } else {
        format!(
            " Find file: {} ({}/{}) ",
            finder.branch,
            finder.list.matches.len(),
            finder.list.items.len()
        )
    };
    let inner = render_modal_frame(f, area, title, th);

    // First row: the always-on fuzzy query input.
    if let Some(query_area) = row_rect(inner, 0) {
        let line = search_line(&finder.list.query, "type to filter…", query_area, th);
        f.render_widget(Paragraph::new(line), query_area);
    }
    let list_inner = below_first_row(inner);

    if finder.listing.is_some() {
        empty_list_row(f, list_inner, "listing files…", th);
    } else if finder.list.matches.is_empty() {
        empty_list_row(f, list_inner, NO_MATCHES, th);
    }
    let start = finder.list.window_start(list_inner.height as usize);
    for (row, (i, m)) in finder
        .list
        .matches
        .iter()
        .enumerate()
        .skip(start)
        .enumerate()
    {
        let Some(row_area) = row_rect(list_inner, row) else {
            break;
        };
        let path = &finder.list.items[m.item];
        // The FILE ICON ahead of the path, as VS Code's quick open has it.
        let icon = crate::icons::file_span(path, app.chrome.icons, th.light);
        let icon_w = icon.as_ref().map_or(0, |s| s.width());
        let budget = (list_inner.width as usize).saturating_sub(2 + icon_w);
        let shown = truncate(path, budget);
        let positions = visible_positions(&m.positions, &shown, path);
        let mut spans = vec![Span::raw(" ")];
        spans.extend(icon);
        spans.extend(fuzzy_highlight_spans(&shown, positions, th));
        render_row(f, row_area, spans, i == finder.list.cursor, true, th);
    }

    // Write-back (draw works on a clone): rects for mouse
    // hit-testing.
    if let Some(Overlay::Files(fin)) = &mut app.modals.overlay {
        fin.area = area;
        fin.list.list_area = list_inner;
        fin.list.sync_scroll(list_inner.height as usize);
    }
}

fn draw_grep_overlay(f: &mut Frame, app: &mut App, view: crate::app::GrepView) {
    let th = app.chrome.theme;
    let area = centered_rect_pct(f.area(), GREP_MODAL_PCT.0, GREP_MODAL_PCT.1);
    let title = if view.list.query.chars().count() < crate::grep_search::MIN_QUERY_LEN {
        format!(" Find in files: {} ", view.branch)
    } else if view.waiting.is_some() {
        format!(" Find in files: {} (searching…) ", view.branch)
    } else if view.truncated {
        format!(
            " Find in files: {} ({}+ hits) ",
            view.branch,
            view.list.items.len()
        )
    } else {
        format!(
            " Find in files: {} ({} hits) ",
            view.branch,
            view.list.items.len()
        )
    };
    let inner = render_modal_frame(f, area, title, th);

    // First row: the always-live grep query.
    if let Some(query_area) = row_rect(inner, 0) {
        let line = search_line(&view.list.query, "type to search…", query_area, th);
        f.render_widget(Paragraph::new(line), query_area);
    }
    let list_inner = below_first_row(inner);

    // Placeholder row: error, too-short query, or an empty result.
    let placeholder = if let Some(err) = &view.error {
        Some(Span::styled(err.clone(), Style::default().fg(th.err)))
    } else if view.list.query.chars().count() < crate::grep_search::MIN_QUERY_LEN {
        Some(Span::styled(
            format!(
                "type at least {} characters to search",
                crate::grep_search::MIN_QUERY_LEN
            ),
            Style::default().fg(th.dim),
        ))
    } else if view.list.items.is_empty() && view.waiting.is_none() {
        Some(Span::styled(NO_MATCHES, Style::default().fg(th.dim)))
    } else {
        None
    };
    if let (Some(span), Some(row_area)) = (placeholder, row_rect(list_inner, 0)) {
        f.render_widget(Paragraph::new(span), row_area);
    }

    let start = view.list.window_start(list_inner.height as usize);
    for (row, (i, hit)) in view.list.items.iter().enumerate().skip(start).enumerate() {
        let Some(row_area) = row_rect(list_inner, row) else {
            break;
        };
        let budget = (list_inner.width as usize).saturating_sub(2);
        let loc = format!("{}:{}", hit.path, hit.line);
        let loc_len = loc.chars().count();
        let mut spans = vec![Span::raw(" ")];
        if loc_len + 2 >= budget {
            spans.push(Span::styled(
                truncate(&loc, budget),
                Style::default().fg(th.accent),
            ));
        } else {
            spans.push(Span::styled(loc, Style::default().fg(th.accent)));
            spans.push(Span::raw("  "));
            spans.push(Span::raw(truncate(&hit.text, budget - loc_len - 2)));
        }
        render_row(f, row_area, spans, i == view.list.cursor, true, th);
    }

    // Write-back (draw works on a clone): rects for mouse
    // hit-testing.
    if let Some(Overlay::Grep(v)) = &mut app.modals.overlay {
        v.area = area;
        v.list.list_area = list_inner;
        v.list.sync_scroll(list_inner.height as usize);
    }
}

fn draw_hosts_overlay(f: &mut Frame, app: &mut App, view: crate::app::HostsView) {
    let th = app.chrome.theme;
    let total = view.hosts.len();
    let selected = view.selected.min(total.saturating_sub(1));
    let adding = view.input.is_some();
    let list_rows = (total + adding as usize).max(1);
    let height = (list_rows as u16)
        .saturating_add(2)
        .clamp(5, f.area().height.max(5));
    let area = centered_rect(f.area(), HOSTS_W, height);
    f.render_widget(Clear, area);
    let hint = if adding {
        " type user@host [dir]  Enter: connect  Esc: cancel "
    } else {
        " Enter: connect  a: new host  d: remove  Esc: close "
    };
    let block = modal_block(" SSH Hosts ", th)
        .title_bottom(Line::from(Span::styled(hint, Style::default().fg(th.dim))));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if total == 0 && !adding {
        empty_list_row(f, inner, "no hosts yet, a connects to a new one", th);
    }
    // Follow-window keeps the cursor visible; while adding, pin the
    // window to the tail so the input row is always on screen.
    let start = if adding {
        list_rows.saturating_sub(inner.height as usize)
    } else {
        view.window_start(inner.height as usize)
    };
    let now = nebula_core::clock::now_ms();
    for (i, entry) in view.hosts.iter().enumerate().skip(start) {
        let Some(row_area) = row_rect(inner, i - start) else {
            break;
        };
        let budget = (inner.width as usize).saturating_sub(2);
        // "host  dir" left, a dim "2h ago" pinned right.
        let ago = if entry.last_used_ms > 0 {
            crate::hosts::ago_label(now - entry.last_used_ms)
        } else {
            String::new()
        };
        let ago_w = ago.chars().count();
        let text_budget = budget.saturating_sub(if ago_w > 0 { ago_w + 2 } else { 0 });
        let host_txt = truncate(&entry.host, text_budget);
        let mut used = host_txt.chars().count();
        let mut spans = vec![Span::raw(host_txt)];
        if let Some(p) = &entry.path {
            if used + 2 < text_budget {
                let dir = truncate(&format!("  {p}"), text_budget - used);
                used += dir.chars().count();
                spans.push(Span::styled(dir, Style::default().fg(th.dim)));
            }
        }
        if ago_w > 0 && used + ago_w < budget {
            spans.push(Span::raw(" ".repeat(budget - used - ago_w)));
            spans.push(Span::styled(ago, Style::default().fg(th.dim)));
        }
        render_row(f, row_area, spans, i == selected && !adding, true, th);
    }
    if let Some(input) = &view.input {
        if let Some(row_area) = row_rect(inner, total.saturating_sub(start)) {
            let budget = (inner.width as usize).saturating_sub(2);
            let mut spans = vec![Span::styled("+ ", Style::default().fg(th.accent))];
            spans.extend(input_spans(input, budget, th.accent, th));
            f.render_widget(Paragraph::new(Line::from(spans)), row_area);
        }
    }

    // Write-back (draw works on a clone): rects for mouse
    // hit-testing, plus the clamped cursor.
    if let Some(Overlay::Hosts(v)) = &mut app.modals.overlay {
        v.area = area;
        v.list_area = inner;
        v.selected = selected;
    }
}

fn draw_agent_presets_overlay(
    f: &mut Frame,
    app: &mut App,
    view: crate::preset_overlays::AgentPresetsView,
) {
    let th = app.chrome.theme;
    crate::preset_overlays::draw_list(f, app, &view, th);
}

fn draw_agent_preset_editor_overlay(
    f: &mut Frame,
    app: &mut App,
    editor: crate::preset_overlays::AgentPresetEditor,
) {
    let th = app.chrome.theme;
    crate::preset_overlays::draw_editor(f, app, &editor, th)
}

fn draw_issues_overlay(f: &mut Frame, app: &mut App, view: crate::issues::IssuesView) {
    let th = app.chrome.theme;
    crate::issues::draw(f, app, &view, th, false);
}

fn draw_pull_requests_overlay(
    f: &mut Frame,
    app: &mut App,
    view: crate::pr_modal::PullRequestsView,
) {
    let th = app.chrome.theme;
    crate::pr_modal::draw(f, app, &view, th, false);
}

fn draw_branch_switch_overlay(
    f: &mut Frame,
    app: &mut App,
    view: crate::branch_switch::BranchSwitchView,
) {
    let th = app.chrome.theme;
    crate::branch_switch::draw(f, app, &view, th);
}

fn draw_file_tabs_overlay(f: &mut Frame, app: &mut App, mut view: crate::file_tabs::FileTabsView) {
    let th = app.chrome.theme;
    // The TREE BROWSER's footprint: the editor Enter opens wants the
    // room, and the preview is a whole file.
    let area = centered_rect_pct(f.area(), SPLIT_MODAL_PCT.0, SPLIT_MODAL_PCT.1);
    let title = format!(" Open files ({}) ", view.tabs.len());
    let inner = render_modal_frame(f, area, title, th);

    // ---- tab strip and its rule ----
    let (strip, hits) = tab_strip(
        inner.x,
        view.tabs.iter().map(|t| {
            let path = t.path.to_string_lossy();
            (
                t.label.as_str(),
                crate::icons::file_span(&path, app.chrome.icons, th.light),
            )
        }),
        view.tab,
        view.on_tabs,
        th,
    );
    let head = Rect {
        height: inner.height.min(2),
        ..inner
    };
    f.render_widget(
        Paragraph::new(vec![Line::from(strip), strip_rule(inner.width, th)]),
        head,
    );

    // ---- body: the preview, or the embedded editor draw_vim paints
    // over it after us ----
    let body = Rect {
        x: inner.x,
        y: inner.y.saturating_add(2),
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let editing = app.pane.vim.as_ref().is_some_and(|v| v.embedded);
    let graphics_mode = crate::config::Config::load().graphics_mode();
    let visual_lines = (!editing)
        .then(|| {
            view.visual
                .as_mut()
                .map(|visual| visual.render(body, graphics_mode.clone(), th))
        })
        .flatten();
    // A markdown tab shows the rendered page — flowed for this
    // width, kept on the view between draws — unless `m` asked
    // for the source. No gutter: rendered rows aren't source lines.
    let rendered = (view.renders_markdown() && !editing && visual_lines.is_none()).then(|| {
        crate::markdown::Rendered::for_width_with_diagrams(
            view.rendered.take(),
            &view.preview_text,
            body.width,
            crate::markdown::Breaks::Reflow,
            th,
            &mut view.mermaid_diagrams,
            graphics_mode.clone(),
        )
    });
    let line_count = match (&visual_lines, &rendered) {
        (Some(lines), _) => lines.len(),
        (None, Some(r)) => r.lines.len(),
        (None, None) => view.preview_lines.len(),
    };
    let max_scroll = line_count
        .saturating_sub(body.height as usize)
        .min(u16::MAX as usize) as u16;
    let scroll = view.scroll.min(max_scroll);
    if !editing && body.height > 0 {
        let lines = match (&visual_lines, &rendered) {
            (Some(lines), _) => lines
                .iter()
                .skip(scroll as usize)
                .take(body.height as usize)
                .cloned()
                .collect(),
            (None, Some(r)) => r
                .lines
                .iter()
                .skip(scroll as usize)
                .take(body.height as usize)
                .cloned()
                .collect(),
            (None, None) => preview_window(
                &view.preview_lines,
                line_count,
                view.preview_is_file,
                scroll,
                body,
                th,
            ),
        };
        f.render_widget(Paragraph::new(lines), body);
    }

    // ---- keys hint on the last row ----
    if let Some(hint_area) = row_rect(inner, inner.height.saturating_sub(1) as usize) {
        f.render_widget(
            Paragraph::new(Span::styled(
                truncate(
                    &format!(" {}", file_tabs_keys_hint(&view, editing)),
                    inner.width as usize,
                ),
                Style::default().fg(th.dim),
            )),
            hint_area,
        );
    }

    // Write-back (draw works on a clone): hit rects for the mouse,
    // the pane for the embedded editor, the page size for paging,
    // the scroll re-clamped so resizes never strand the view, the
    // shown line count and the flowed page for the next draw.
    if let Some(Overlay::FileTabs(v)) = &mut app.modals.overlay {
        v.area = area;
        v.tab_hits = hits;
        v.body_area = body;
        v.view_height = body.height;
        v.scroll = scroll;
        v.preview_line_count = line_count;
        if rendered.is_some() {
            v.rendered = rendered;
        }
        v.visual = view.visual;
        v.mermaid_diagrams = view.mermaid_diagrams;
    }
}

fn draw_tree_overlay(f: &mut Frame, app: &mut App, mut view: crate::tree_browser::TreeBrowser) {
    let th = app.chrome.theme;
    let area = centered_rect_pct(f.area(), SPLIT_MODAL_PCT.0, SPLIT_MODAL_PCT.1);
    f.render_widget(Clear, area);
    // Cap first, floor second: on a tiny screen the tree keeps its
    // minimum and SPLIT_PANE_LAYOUT_MIN squeezes the preview pane
    // instead.
    let files_w = view
        .files_width
        .min(area.width.saturating_sub(crate::app::MIN_DIFF_PANE_W))
        .max(crate::app::MIN_DIFF_FILES_W);
    let [tree_a, preview_a] = Layout::horizontal([
        Constraint::Length(files_w),
        Constraint::Min(SPLIT_PANE_LAYOUT_MIN),
    ])
    .areas(area);

    // Left: the file tree; a stateless follow-window keeps the
    // selected row visible.
    let tree_title = if view.listing.is_some() {
        format!("Tree: {} (listing…)", view.branch)
    } else if view.list.query.is_empty() {
        format!("Tree: {} ({})", view.branch, view.file_count)
    } else {
        format!(
            "Tree: {} ({}/{})",
            view.branch, view.match_count, view.file_count
        )
    };
    let block = panel_block(&tree_title, true, th);
    let tree_inner = block.inner(tree_a);
    f.render_widget(block, tree_a);

    // First row: the always-on fuzzy filter input.
    if let Some(filter_area) = row_rect(tree_inner, 0) {
        let line = search_line(&view.list.query, "type to filter…", filter_area, th);
        f.render_widget(Paragraph::new(line), filter_area);
    }
    let list_inner = below_first_row(tree_inner);

    if view.listing.is_some() {
        empty_list_row(f, list_inner, "listing files…", th);
    } else if view.list.matches.is_empty() {
        empty_list_row(f, list_inner, NO_MATCHES, th);
    }
    let start = view.list.window_start(list_inner.height as usize);
    for (row, (i, m)) in view.list.matches.iter().enumerate().skip(start).enumerate() {
        let Some(row_area) = row_rect(list_inner, row) else {
            break;
        };
        let r = &view.list.items[m.item];
        let node = &view.nodes[r.node];
        let indent = "  ".repeat(node.depth);
        // Directories fold; a live filter forces them all open.
        let marker = if !node.is_dir {
            "  "
        } else if !view.list.query.is_empty() || view.expanded[r.node] {
            "▾ "
        } else {
            "▸ "
        };
        // The FILE ICON between the fold marker and the name, as VS
        // Code's explorer draws it: the folder's own (open or folded),
        // or the file type's.
        let open = !view.list.query.is_empty() || view.expanded[r.node];
        let icon = if node.is_dir {
            crate::icons::folder_span(&node.name, open, app.chrome.icons, th.light)
        } else {
            crate::icons::file_span(&node.name, app.chrome.icons, th.light)
        };
        let icon_w = icon.as_ref().map_or(0, |s| s.width());
        let budget =
            (list_inner.width as usize).saturating_sub(indent.chars().count() + 3 + icon_w);
        let shown = truncate(&node.name, budget);
        let positions = visible_positions(&r.positions, &shown, &node.name);
        let mut spans = vec![
            Span::raw(format!(" {indent}")),
            Span::styled(marker, Style::default().fg(th.accent)),
        ];
        spans.extend(icon);
        if node.is_dir {
            spans.push(Span::styled(shown, Style::default().fg(th.accent)));
        } else {
            spans.extend(fuzzy_highlight_spans(&shown, positions, th));
        }
        render_row(f, row_area, spans, i == view.list.cursor, true, th);
    }

    // Right: the selected node's preview, syntax-highlighted and
    // scrolled — or the embedded editor, which draw_vim paints into
    // this pane after us.
    let editing = app.pane.vim.as_ref().is_some_and(|v| v.embedded);
    let sel_path = view.selected_node().map(|n| n.path.as_str()).unwrap_or("");
    let title = if editing {
        format!(
            "{}: editing",
            truncate(sel_path, (preview_a.width as usize).saturating_sub(14))
        )
    } else {
        truncate(sel_path, (preview_a.width as usize).saturating_sub(4))
    };
    let mut block = panel_block(&title, true, th);
    let preview_inner = block.inner(preview_a);
    let graphics_mode = crate::config::Config::load().graphics_mode();
    let visual_lines = (!editing)
        .then(|| {
            view.visual
                .as_mut()
                .map(|visual| visual.render(preview_inner, graphics_mode.clone(), th))
        })
        .flatten();
    // A markdown file shows the rendered page (the FILE TABS'
    // rule), flowed for this width and kept between draws, unless
    // Ctrl+r asked for the source.
    let rendered = (view.renders_markdown() && !editing && visual_lines.is_none()).then(|| {
        crate::markdown::Rendered::for_width_with_diagrams(
            view.rendered.take(),
            &view.preview,
            preview_inner.width,
            crate::markdown::Breaks::Reflow,
            th,
            &mut view.mermaid_diagrams,
            graphics_mode.clone(),
        )
    });
    let line_count = match (&visual_lines, &rendered) {
        (Some(lines), _) => lines.len(),
        (None, Some(r)) => r.lines.len(),
        (None, None) => view.preview_lines.len(),
    };
    let max_scroll =
        (line_count.min(u16::MAX as usize) as u16).saturating_sub(preview_inner.height.max(1));
    let scroll = view.scroll.min(max_scroll);
    if !editing && max_scroll > 0 {
        block = block.title_bottom(
            Line::from(Span::styled(
                format!(" {}/{} ", scroll + 1, line_count),
                Style::default().fg(th.dim),
            ))
            .right_aligned(),
        );
    }
    f.render_widget(block, preview_a);
    if !editing {
        // Line-number gutter, for real file contents only —
        // directory listings and placeholders have no lines to
        // number, and a rendered page's rows aren't source lines.
        // Dropped entirely when the pane is too narrow to leave
        // room for the code itself.
        let lines = match (&visual_lines, &rendered) {
            (Some(lines), _) => lines
                .iter()
                .skip(scroll as usize)
                .take(preview_inner.height as usize)
                .cloned()
                .collect(),
            (None, Some(r)) => r
                .lines
                .iter()
                .skip(scroll as usize)
                .take(preview_inner.height as usize)
                .cloned()
                .collect(),
            (None, None) => preview_window(
                &view.preview_lines,
                line_count,
                view.preview_is_file,
                scroll,
                preview_inner,
                th,
            ),
        };
        f.render_widget(Paragraph::new(lines), preview_inner);
    }

    // Write-back (draw works on a clone): page size for key paging,
    // scroll re-clamped so resizes never strand the view, preview
    // rect for the embedded editor.
    if let Some(Overlay::Tree(v)) = &mut app.modals.overlay {
        v.view_height = preview_inner.height;
        v.scroll = scroll;
        v.preview_line_count = line_count;
        if rendered.is_some() {
            v.rendered = rendered;
        }
        v.visual = view.visual;
        v.mermaid_diagrams = view.mermaid_diagrams;
        v.list.list_area = list_inner;
        v.list.sync_scroll(list_inner.height as usize);
        v.preview_area = preview_inner;
        v.area = area;
        v.files_width = files_w;
    }
}

/// The REVIEW UNIT's verb row under its docked diff: each verb a button
/// naming its key (`Space reply`, `⌥p open PR`…), in the room there is,
/// the ones that do not fit left off the end. Returns each one's cells.
fn draw_review_verbs(
    f: &mut Frame,
    app: &App,
    view: &crate::app::DiffView,
    row: Rect,
) -> Vec<(Rect, crate::app::ReviewVerb)> {
    let th = app.chrome.theme;
    f.render_widget(Clear, row);
    let Some(dock) = &view.review else {
        return Vec::new();
    };
    let hover = app.modals.button_hover;
    let mut x = row.x;
    let mut out = Vec::new();
    let mut spans = Vec::new();
    for (verb, key, word) in crate::event_loop::review_verbs() {
        let word = match verb {
            crate::app::ReviewVerb::Pr => crate::event_loop::review_pr_word(app, &dock.worktree),
            _ => word,
        };
        let text = format!(" {key} {word} ");
        let w = text.chars().count() as u16;
        if x + w > row.right() {
            break;
        }
        let rect = Rect {
            x,
            y: row.y,
            width: w,
            height: 1,
        };
        let destructive = matches!(verb, crate::app::ReviewVerb::DeleteWorktree);
        let mut style = Style::default().fg(if destructive { th.err } else { th.text });
        if hover == Some(rect) {
            style = style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
        }
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
        out.push((rect, verb));
        x += w + 1;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row);
    out
}

#[cfg(test)]
mod help_tests {
    use super::{HelpKeys, HELP_LEFT, HELP_RIGHT};

    /// The HELP lists every action a key can be bound to — but the PROJECT
    /// TAB slots, which share the one `1-9` row — so nothing is reachable
    /// only by a key nobody was told about, and every row is a button.
    #[test]
    fn help_lists_every_action() {
        let listed: Vec<crate::keymap::Action> = HELP_LEFT
            .iter()
            .chain(HELP_RIGHT)
            .flat_map(|(_, rows)| rows.iter())
            .filter_map(|(keys, _)| match keys {
                HelpKeys::Act(actions) => Some(actions.iter().copied()),
                HelpKeys::Lit(_) => None,
            })
            .flatten()
            .collect();
        let missing: Vec<&str> = crate::keymap::ACTIONS
            .iter()
            .filter(|spec| !matches!(spec.action, crate::keymap::Action::SelectProjectTab(_)))
            // The menus' own verbs ship unbound: the menus and `?` list
            // them, with a key once one is bound, and Hotkeys binds them.
            .filter(|spec| !spec.defaults.is_empty())
            .filter(|spec| !listed.contains(&spec.action))
            .map(|spec| spec.id)
            .collect();
        assert!(missing.is_empty(), "not in the help: {missing:?}");
    }
}
