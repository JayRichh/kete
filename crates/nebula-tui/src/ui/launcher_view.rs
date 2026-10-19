//! The LAUNCHER VIEW's drawing (`crate::launcher` is its model,
//! `event_loop::launcher` its keys): the GRID of session cards — each card
//! the session's name and status, gathered into a BAND per worktree whose
//! rule wears the branch and its pull request — beside the PANE that reads
//! the card under the cursor (`ui::draw` splits the body and fills that pane);
//! plus the view's own pieces of the QUICK PROMPT (the project on its
//! target row, its key hints) and the PROJECT PICKER.

use super::{
    ago_badge, below_first_row, centered_rect, empty_list_row, fit_ago, fuzzy_highlight_spans,
    over_box_rect, render_modal_frame, render_row, row_rect, search_line, status_dot,
    status_name_spans, sweep_ramp, truncate, visible_positions, NO_MATCHES, OVER_BOX_INSET,
    PENDING_SESSION_BADGE,
};
use crate::app::{App, Focus, HitTarget, Overlay};
use crate::launcher::{BoxField, Hidden, LauncherRow, ProjectPicker, ProjectTab, Tally};
use crate::quick_prompt::QuickLaunch;
use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};
use ratatui::Frame;

mod columns;
mod kanban;
mod nested;

/// Width of the view's QUICK PROMPT, and its height: wider and taller than
/// the panels' box, since here it is the front door. The extra row over
/// the panels' box pays for the blank one between the details and the
/// question, so the editor keeps its full height.
pub(super) const BOX_SIZE: (u16, u16) = (92, 18);
/// The PROJECT PICKER's width, and the most rows it lists before scrolling.
const PICKER_W: u16 = 64;
const PICKER_ROWS: u16 = 14;
/// What a full-screen session's back button says — the word a click on
/// it goes back to.
const CRUMB: &str = "sessions";
/// Longest a project's name is drawn on its PROJECT TAB before it is
/// clipped, so one long name cannot push every other tab off the row.
const PROJECT_TAB_MAX: usize = 20;
/// The lit tab's name is never cut under this: at that point it gives
/// way whole, and the MORE CHIP counts it with the rest.
const TAB_NAME_MIN: usize = 3;
/// The air between the PROJECT TABS and the counts right of them.
const HEAD_GAP: usize = 2;
/// The button before the first PROJECT TAB, and what it says with no tab
/// beside it — the one time the header has room to say what it does.
/// A column of air either side is part of the button, so the pointer has
/// more than one cell to find.
const ADD: &str = "+";
const ADD_EMPTY: &str = "+ open a project";

/// The view's area, with the view on: the PROJECT TABS header, then the
/// GRID of the lit project's session cards under it. `body` is what
/// `crate::launcher::split` left over the PANE along the bottom, which
/// `ui::draw` fills with the card under the cursor; Enter on a session
/// card steps down into that pane (`event_loop::launcher::enter_pane`);
/// on a body too short to draw one it full-screens the session over the
/// lot instead (`event_loop::launcher::open_session`, the `collapsed` arm
/// of `ui::draw`).
pub(super) fn draw(f: &mut Frame, app: &mut App, body: Rect) {
    draw_view(f, app, body);
    draw_drop_marks(f, app);
}

/// Where a held card, band or tab would land, marked over the frame just
/// drawn in the theme's accent: a 1px rule in the gap beside the card it would
/// sit next to, the rule of the band it would go before (`▲`) or after
/// (`▼`), a bar at the side of the tab it would go beside. Read off the
/// frame's own hit map, so the mark is where the cells are.
fn draw_drop_marks(f: &mut Frame, app: &App) {
    use crate::app::{OrderItem, OrderOver};
    use crate::order::Place;
    let th = app.chrome.theme;
    let accent = Style::default().fg(th.accent).add_modifier(Modifier::BOLD);
    let frame = f.area();
    let buf = f.buffer_mut();
    // A bar one column wide down `r`'s height at `x`, kept on the frame.
    let mut bar = |x: u16, r: Rect| {
        if x < frame.right() {
            for y in r.y..r.bottom().min(frame.bottom()) {
                buf.set_string(x, y, "│", accent);
            }
        }
    };
    let beside = |r: Rect, place: Place| match place {
        Place::Before => r.x.saturating_sub(1),
        Place::After => r.right(),
    };
    if let Some((target, place)) = app
        .launcher
        .card_drag
        .as_ref()
        .filter(|d| d.active)
        .and_then(|d| d.slot.clone())
    {
        let bands = crate::launcher::bands(app);
        let sref = nebula_core::SessionRef::Agent(target);
        let rect = app.chrome.hits.iter().find_map(|(r, hit)| match hit {
            HitTarget::LauncherCard(at)
                if crate::launcher::card_at(&bands, *at).is_some_and(|c| c.sref() == sref) =>
            {
                Some(*r)
            }
            _ => None,
        });
        if let Some(r) = rect {
            bar(beside(r, place), r);
        }
        return;
    }
    let Some(drag) = app.launcher.order_drag.as_ref().filter(|d| d.active) else {
        return;
    };
    match (&drag.item, &drag.over) {
        (OrderItem::Tab(_), Some(OrderOver::Tab(id, place))) => {
            if let Some(r) = app.hit_rect(&HitTarget::LauncherTab(id.clone())) {
                bar(beside(r, *place), r);
            }
        }
        (OrderItem::Band(_), Some(OrderOver::Band(w, place))) => {
            let bands = crate::launcher::bands(app);
            let rule = app.chrome.hits.iter().find_map(|(r, hit)| match hit {
                HitTarget::LauncherBand(i) if bands.get(*i).is_some_and(|b| &b.worktree == w) => {
                    Some(*r)
                }
                _ => None,
            });
            if let Some(r) = rule {
                let mark = match place {
                    Place::Before => "▲ before ",
                    Place::After => "▼ after ",
                };
                buf.set_stringn(r.x, r.y, mark, usize::from(r.width), accent);
                let from = r.x + mark.chars().count() as u16;
                for x in from..r.right().min(frame.right()) {
                    if let Some(cell) = buf.cell_mut((x, r.y)) {
                        cell.set_fg(th.accent);
                    }
                }
            }
        }
        _ => {}
    }
}

fn draw_view(f: &mut Frame, app: &mut App, body: Rect) {
    app.chrome.body_area = body;
    app.settle_launcher_focus();
    // The project the grid is on gets its tab, whichever way it was
    // opened, before the header lays the tabs out.
    app.settle_project_tabs();
    if app.launcher.launcher_kanban {
        kanban::draw(f, app, body);
        return;
    }
    if app.launcher.launcher_columns {
        columns::draw(f, app, body);
        return;
    }
    // Threads nobody has opened or folded yet: the one under the cursor
    // starts open, the rest collapsed. Before the layout, so this frame
    // draws what `j`/`k` will walk.
    app.classify_nested_threads();
    let bands = crate::launcher::bands(app);
    // The NESTED layout's DETAIL STRIP, pinned to the bottom with a row of
    // air over it: the list scrolls in what is left, and the keys walk
    // the same list (`App::body_area`).
    let (body, strip) = nested_strip_split(app, body, &bands);
    app.chrome.body_area = body;
    let g = crate::launcher::bands_layout(body);
    let cursor = wearing(app, crate::launcher::band_cursor(app, &bands));
    let count = HeadCount::of(&bands);
    if bands.is_empty() {
        draw_head(f, app, body, count, Hidden::default());
        // Nothing archived is not nothing at all: the hero's "type a
        // task" would be advice about the wrong list, so the ARCHIVED
        // VIEW gets the plain empty line and the way back out of it.
        if app.launcher.show_archived {
            let what = format!(
                "nothing archived in this project: {} back to the live sessions",
                crate::ui::key_hint(app, crate::keymap::Action::ToggleArchived)
            );
            draw_list_empty(f, app, g.area, &what);
        } else {
            draw_empty(f, app, g.area);
        }
        return;
    }
    // The ACCORDION: at most one band's cards wrapped into rows in place
    // (`App::launcher_expanded`) — pushing the bands after it down, so
    // the whole panel, not just that band, may run taller than the
    // screen and scrolls as one list.
    // In the compact LIST every band is its entries stacked a line
    // apiece instead (Settings → Appearance → **Worktree layout**), and
    // with **Expand all worktrees** on every band is open at once. The
    // NESTED layout is the third: a thread of one-line rows per worktree
    // over a DETAIL STRIP, drawn by its own module.
    let panel = app.panel_layout(&bands);
    let scroll = settle_panel_scroll(app, &panel, &bands, cursor);
    draw_head(f, app, body, count, panel.hidden(scroll));
    if app.launcher.launcher_nested {
        nested::draw_bands(f, app, &g, &panel, &bands, cursor, scroll);
        if let Some(strip) = strip {
            nested::draw_strip(f, app, strip, &bands, cursor);
        }
    } else {
        draw_bands(f, app, &g, &panel, &bands, cursor, scroll);
    }
}

/// Fewest list rows the NESTED layout keeps over its DETAIL STRIP: a body
/// shorter than that drops the strip rather than the list.
const STRIP_MIN_LIST: u16 = 3;

/// `body` less the NESTED layout's DETAIL STRIP, and where the strip goes:
/// its last [`nested::STRIP_H`] rows, a row of air above them, as wide as
/// the rows' accent bar to their right edge. The whole body, and no strip,
/// in the other layouts, on an empty grid, or on a body too short to keep
/// a few rows of the list over it.
fn nested_strip_split(
    app: &App,
    body: Rect,
    bands: &[crate::launcher::Band],
) -> (Rect, Option<Rect>) {
    use crate::launcher::{HEAD_H, PAD_X};
    let taken = nested::STRIP_H + 1;
    if !app.launcher.launcher_nested
        || bands.is_empty()
        || body.height < HEAD_H + taken + STRIP_MIN_LIST
    {
        return (body, None);
    }
    let strip = Rect {
        x: body.x + PAD_X - 1,
        y: body.bottom() - nested::STRIP_H,
        width: body.width.saturating_sub((PAD_X - 1) * 2),
        height: nested::STRIP_H,
    };
    (
        Rect {
            height: body.height - taken,
            ..body
        },
        Some(strip),
    )
}

/// The scroll this frame draws the GRID at, settled from what the last
/// frame left and what has happened since — the rule
/// [`App::launcher_scroll`] spells out. Opening or closing a band as the
/// ACCORDION starts from the top again rather than wherever the last
/// layout's scroll was left; a scroll the wheel set is kept unless a key
/// asked for the cursor's card ([`App::launcher_reveal`]) or the cursor
/// is on another card than the one it was set under; any other scroll
/// keeps in step with the cursor — its band whole on screen, or on the
/// open band its card with the row over it
/// ([`crate::launcher::PanelLayout::reveal`]). Whatever it lands on is
/// held within the panel, so a window that grew or a list that shrank
/// never scrolls past the last row.
fn settle_panel_scroll(
    app: &mut App,
    panel: &crate::launcher::PanelLayout,
    bands: &[crate::launcher::Band],
    cursor: Option<usize>,
) -> u16 {
    if app.launcher.launcher_scroll_in != app.launcher.launcher_expanded {
        app.launcher.launcher_scroll = 0;
        app.launcher.launcher_scroll_held = false;
        app.launcher.launcher_scroll_in = app.launcher.launcher_expanded.clone();
    }
    let mut scroll = panel.clamp(app.launcher.launcher_scroll);
    if let Some(index) = cursor {
        let band = &bands[index];
        let at = crate::launcher::card_cursor(app, band);
        let on = at.and_then(|i| band.cards.get(i)).map(|c| c.sref());
        if app.launcher.launcher_reveal
            || !app.launcher.launcher_scroll_held
            || app.launcher.launcher_scroll_on != on
        {
            scroll = panel.reveal(scroll, index, at);
            app.launcher.launcher_scroll_held = false;
            app.launcher.launcher_scroll_on = on;
        }
    }
    app.launcher.launcher_reveal = false;
    app.launcher.launcher_scroll = scroll;
    scroll
}

/// What the header counts: the cards on the grid, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct HeadCount {
    sessions: usize,
    terminals: usize,
}

impl HeadCount {
    fn of(bands: &[crate::launcher::Band]) -> Self {
        Self {
            sessions: bands.iter().map(|b| b.sessions()).sum(),
            terminals: bands.iter().map(|b| b.terminals()).sum(),
        }
    }
}

/// The header over the grid: the PROJECT TABS on the left, each with its
/// status dots, how many cards the grid holds on the right — and how many
/// of them it could not fit — a rule under both: the same three-row head
/// the panels' columns sit on.
///
/// The row is the project bar first: the tabs are laid out before
/// anything else and take what they need of it, and the counts on the
/// right get what they leave ([`head_count`]), giving way a piece at a
/// time — so a narrow window, or the PANE down the right of the cards,
/// costs the counts before it costs a project's name.
fn draw_head(f: &mut Frame, app: &mut App, body: Rect, count: HeadCount, hidden: Hidden) {
    let th = app.chrome.theme;
    if let Some(r) = row_rect(body, 1) {
        let r = pad_x(r);
        let tabs = head_tabs(app, r);
        let taken: usize = tabs.iter().map(|s| s.width()).sum();
        f.render_widget(Paragraph::new(Line::from(tabs)), r);
        let room = (r.width as usize).saturating_sub(taken + HEAD_GAP);
        let right = head_count(app, count, hidden, room, th);
        let used: usize = right.iter().map(|(s, _)| s.width()).sum();
        // The PR & ISSUE COUNTS are buttons, laid down where the
        // right-aligned row puts each word: a click opens that list for
        // the project in front of you, as `v` and `i` do.
        let mut x = r.x + (r.width as usize).saturating_sub(used) as u16;
        let mut spans = Vec::with_capacity(right.len());
        for (span, hit) in right {
            let width = span.width() as u16;
            if let Some(hit) = hit {
                app.chrome.hits.push((Rect { x, width, ..r }, hit));
            }
            x += width;
            spans.push(span);
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).alignment(ratatui::layout::Alignment::Right),
            r,
        );
        // BACK / FORWARD and the ATTENTION STRIP, in what the tabs and
        // the counts leave between them.
        let gap = (r.width as usize).saturating_sub(taken + used + HEAD_GAP * 2);
        super::attention_view::draw(
            f,
            app,
            Rect {
                x: r.x + (taken + HEAD_GAP) as u16,
                width: gap as u16,
                ..r
            },
        );
    }
    draw_rule(f, body, 2, th.edge);
}

/// The header's PROJECT TABS: one tab per project opened since its tab
/// was last closed, behind a `+` for the rest, the most recently opened
/// next to it ([`crate::launcher::project_tabs`]) — where the `+` puts
/// the project it opens. The tab the grid is on is lit — a raised chip,
/// its name in the accent — and a click on any other opens it, as `[`
/// and `]` walking onto it do (`event_loop::launcher::open_tab`). The `×`
/// on a tab closes it
/// (`event_loop::launcher::close_tab`) — all but the last one, which is
/// the project on screen and has nowhere to hand the grid to; the `+`
/// drops the PROJECT DROPDOWN under itself, every project narrowed by
/// whatever you type plus a row that opens a folder, and the pick opens a
/// tab (`event_loop::launcher::open_project_menu`). A right-click on a
/// tab opens that project with its menu over it.
///
/// Each tab carries its project's STATUS DOTS after the name — waiting on
/// you (red), finished unread (blue), working (yellow), each with its
/// count and no word ([`tab_dots`]) — so a project that wants you says so
/// from the header, whichever project the grid is on. Its name sweeps on
/// the loudest of the three ([`tab_ramp`]), so the tab says it in motion
/// too.
///
/// The hit rects are laid down as the spans are measured, so a click
/// lands on the tab itself and never on the air between two, and the
/// dropdown hangs off the `+`'s own rect.
///
/// With the keys up here (`k`,`k` off the top row of cards — see
/// [`App::launcher_tab_cursor`]) the header has a cursor of its own: the
/// tab it is on wears the accent as a solid block, the way a focused
/// title chip does, whichever tab is lit.
///
/// The tabs get the whole row ([`draw_head`]) and give way in steps, the
/// lit tab last of all ([`fit_tabs`]): first the `×` on every tab but the
/// lit one — the browser's own answer to a crowded tab strip, the lit tab
/// being the one a close is aimed at — then whole tabs, from the right,
/// into the MORE CHIP after the last one drawn (`2 more ▾`, [`more_chip`]),
/// which carries their STATUS DOTS and sweeps as a tab would, so a project
/// off the row that wants you still says so from the header. Its click
/// lists them (`event_loop::launcher::open_more_tabs_menu`). The lit tab
/// is always drawn, whole wherever there is room for it, so the header
/// always says which project the grid is on.
fn head_tabs(app: &mut App, r: Rect) -> Vec<Span<'static>> {
    let th = app.chrome.theme;
    let hover = app.launcher.hover_crumb.clone();
    let sweep = app.chrome.animations.then(|| app.sweep_phase());
    let tabs = crate::launcher::project_tabs(app);
    let room = r.width as usize;
    let add = if tabs.is_empty() { ADD_EMPTY } else { ADD };
    let add_w = add.chars().count() + 2;
    // The `+` is laid out first: it is the only way to a project with no
    // tab, so the tabs shrink around it rather than push it off the row.
    let budget = room.saturating_sub(add_w + 1);
    let chip =
        |tab: &ProjectTab, name_max: usize| project_chip(tab, name_max, hover.as_ref(), sweep, th);
    let sizes: Vec<TabSize> = tabs
        .iter()
        .map(|tab| {
            let [label, cross] = chip(tab, PROJECT_TAB_MAX);
            TabSize {
                label: label.width(),
                cross: cross.width(),
                name: tab.name.chars().count().min(PROJECT_TAB_MAX),
            }
        })
        .collect();
    let lit = tabs
        .iter()
        .position(|t| t.focused)
        .or_else(|| tabs.iter().position(|t| t.active));
    let fit = fit_tabs(&sizes, lit, budget, |more| {
        more_chip(more.len(), tally_of(&tabs, more), false, None, th).width()
    });
    app.launcher.launcher_tabs_more = fit.more.iter().map(|&i| tabs[i].id.clone()).collect();

    // The `+` leads the row, on the side a project it opens lands on: a
    // button after the last tab would read as appending one there.
    let mut row: Vec<PaneTab> = Vec::new();
    if add_w <= room {
        let mut style = Style::default().fg(th.muted);
        if hover.as_ref() == Some(&HitTarget::LauncherTabAdd) {
            style = Style::default()
                .fg(th.accent)
                .add_modifier(Modifier::UNDERLINED);
        }
        row.push(PaneTab {
            spans: vec![Span::raw(" "), Span::styled(add, style), Span::raw(" ")],
            hit: Some(HitTarget::LauncherTabAdd),
        });
    }
    let mut tabs_row: Vec<PaneTab> = Vec::new();
    for (n, &i) in fit.shown.iter().enumerate() {
        if n > 0 {
            tabs_row.push(PaneTab::plain(vec![Span::raw(" ")]));
        }
        let is_lit = Some(i) == lit;
        let name_max = if is_lit {
            fit.lit_name
        } else {
            PROJECT_TAB_MAX
        };
        let [label, cross] = chip(&tabs[i], name_max);
        tabs_row.push(label);
        if fit.crosses || is_lit {
            tabs_row.push(cross);
        }
    }
    if !fit.more.is_empty() {
        if !tabs_row.is_empty() {
            tabs_row.push(PaneTab::plain(vec![Span::raw(" ")]));
        }
        let hovered = hover.as_ref() == Some(&HitTarget::LauncherTabMore);
        tabs_row.push(more_chip(
            fit.more.len(),
            tally_of(&tabs, &fit.more),
            hovered,
            sweep,
            th,
        ));
    }
    if !row.is_empty() && !tabs_row.is_empty() {
        row.push(PaneTab::plain(vec![Span::raw(" ")]));
    }
    row.extend(tabs_row);

    let mut spans = Vec::new();
    let mut x = r.x;
    for tab in row {
        let width = u16::try_from(tab.width()).unwrap_or(u16::MAX);
        if let Some(hit) = tab.hit {
            app.chrome.hits.push((
                Rect {
                    x,
                    width,
                    height: 1,
                    ..r
                },
                hit,
            ));
        }
        x = x.saturating_add(width);
        spans.extend(tab.spans);
    }
    spans
}

/// The columns a PROJECT TAB takes, measured whole: its name and STATUS
/// DOTS (`label`), the `×` beside them (`cross`), and how much of the
/// label is the name itself (`name`) — what the lit tab can give up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TabSize {
    label: usize,
    cross: usize,
    name: usize,
}

/// How the PROJECT TABS share their row ([`fit_tabs`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TabFit {
    /// The tabs drawn, by index, in tab order.
    shown: Vec<usize>,
    /// Every tab drawn keeps its `×`, not just the lit one.
    crosses: bool,
    /// How much of the lit tab's name is drawn.
    lit_name: usize,
    /// The tabs the MORE CHIP stands in for, by index, in tab order.
    more: Vec<usize>,
}

/// Which PROJECT TABS the header draws in `budget` columns, each step
/// giving up less than the next: every tab with its `×`; every tab, the
/// `×` on the lit one only; the lit tab and as many of the rest as fit,
/// in tab order, with the MORE CHIP (`more_w` columns for the tabs it
/// holds) standing in for the others; the lit tab's name cut, down to
/// [`TAB_NAME_MIN`]; the chip alone, holding them all; and nothing. The
/// tabs keep their own order, so the lit tab drawn out of its place
/// sits where it falls among the ones kept, and the ones kept are the
/// first ones — the projects last worked in.
fn fit_tabs(
    sizes: &[TabSize],
    lit: Option<usize>,
    budget: usize,
    more_w: impl Fn(&[usize]) -> usize,
) -> TabFit {
    let all: Vec<usize> = (0..sizes.len()).collect();
    let lit_name = lit.map_or(0, |i| sizes[i].name);
    let row = |shown: &[usize], crosses: bool| -> usize {
        let tabs: usize = shown
            .iter()
            .map(|&i| {
                let cross = crosses || Some(i) == lit;
                sizes[i].label + if cross { sizes[i].cross } else { 0 }
            })
            .sum();
        tabs + shown.len().saturating_sub(1)
    };
    // The chip and the column of air before it; nothing with no chip.
    let chip = |more: &[usize]| {
        if more.is_empty() {
            0
        } else {
            1 + more_w(more)
        }
    };
    for crosses in [true, false] {
        if row(&all, crosses) <= budget {
            return TabFit {
                shown: all,
                crosses,
                lit_name,
                more: Vec::new(),
            };
        }
    }
    let others: Vec<usize> = all.iter().copied().filter(|&i| Some(i) != lit).collect();
    for kept in (0..others.len()).rev() {
        let mut shown = others[..kept].to_vec();
        shown.extend(lit);
        shown.sort_unstable();
        let more = others[kept..].to_vec();
        if row(&shown, false) + chip(&more) <= budget {
            return TabFit {
                shown,
                crosses: false,
                lit_name,
                more,
            };
        }
    }
    if let Some(i) = lit {
        let over = (row(&[i], false) + chip(&others)).saturating_sub(budget);
        if lit_name >= TAB_NAME_MIN + over {
            return TabFit {
                shown: vec![i],
                crosses: false,
                lit_name: lit_name - over,
                more: others,
            };
        }
    }
    let more = if !all.is_empty() && more_w(&all) <= budget {
        all
    } else {
        Vec::new()
    };
    TabFit {
        shown: Vec::new(),
        crosses: false,
        lit_name,
        more,
    }
}

/// The STATUS DOTS the tabs at `ids` would carry between them.
fn tally_of(tabs: &[ProjectTab], ids: &[usize]) -> Tally {
    ids.iter().fold(Tally::default(), |sum, &i| Tally {
        needs_you: sum.needs_you + tabs[i].tally.needs_you,
        done: sum.done + tabs[i].tally.done,
        running: sum.running + tabs[i].tally.running,
    })
}

/// What the MORE CHIP says it drops, after the count.
const MORE_CARET: &str = " ▾ ";

/// The MORE CHIP: ` 2 more ●1 ▾ ` — how many PROJECT TABS the row had no
/// room for, the STATUS DOTS they carry between them, and the caret that
/// says a click lists them. Its words sweep on the loudest of those dots
/// ([`tab_ramp`]) as a tab's name does, so a project off the row that is
/// waiting on you is still seen moving in the header. Underlined under
/// the pointer, as the tabs are.
fn more_chip(
    count: usize,
    tally: Tally,
    hovered: bool,
    sweep: Option<usize>,
    th: Theme,
) -> PaneTab {
    let mut words = Style::default().fg(th.muted);
    if hovered {
        words = words.add_modifier(Modifier::UNDERLINED);
    }
    let ramp = sweep.and_then(|_| tab_ramp(tally, th));
    let mut spans = vec![Span::raw(" ")];
    spans.extend(status_name_spans(
        format!("{count} more"),
        words,
        ramp,
        sweep.unwrap_or(0),
    ));
    spans.extend(tab_dots(tally, th));
    spans.push(Span::styled(MORE_CARET, Style::default().fg(th.dim)));
    PaneTab {
        spans,
        hit: Some(HitTarget::LauncherTabMore),
    }
}

/// One PROJECT TAB, as two hits side by side: the tab — its name and its
/// STATUS DOTS — and the `×` that closes it, a target of its own so a
/// click on the cross never reads as a click on the tab. The lit tab is a
/// raised chip, pads and all, with its name in the accent; the rest sit
/// flat and muted. The name is cut to `name_max`. Every tab closes, the
/// last one included: that one closes to the splash.
///
/// `sweep` is the frame's sweep phase, `None` with the animations off:
/// with it, the name sweeps on the loudest thing its sessions are doing
/// ([`tab_ramp`]) — lit or not, since the tab is what says so from
/// another project. The header's cursor holds still, so the block that
/// says where Enter goes stays legible.
fn project_chip(
    tab: &ProjectTab,
    name_max: usize,
    hover: Option<&HitTarget>,
    sweep: Option<usize>,
    th: Theme,
) -> [PaneTab; 2] {
    let fill = |style: Style| {
        if tab.active {
            style.bg(th.sel_bg)
        } else {
            style
        }
    };
    let mut name = if tab.active {
        Style::default().fg(th.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.muted)
    };
    if hover == Some(&HitTarget::LauncherTab(tab.id.clone())) {
        name = name.add_modifier(Modifier::UNDERLINED);
    }
    // The header's own cursor: the pad and the name as one accent block,
    // so where Enter would go reads apart from which tab is lit.
    let (pad, name) = if tab.focused || tab.drop {
        let cursor = Style::default().bg(th.accent).fg(th.on_accent);
        (cursor, cursor.add_modifier(Modifier::BOLD))
    } else {
        (fill(Style::default()), fill(name))
    };
    let (ramp, phase) = match sweep {
        Some(phase) if !tab.focused => (tab_ramp(tab.tally, th), phase),
        _ => (None, 0),
    };
    let mut label = vec![Span::styled(" ", pad)];
    label.extend(status_name_spans(
        truncate(&tab.name, name_max),
        name,
        ramp,
        phase,
    ));
    label.extend(
        tab_dots(tab.tally, th)
            .into_iter()
            .map(|dot| Span::styled(dot.content, fill(dot.style))),
    );
    let cross = if hover == Some(&HitTarget::LauncherTabClose(tab.id.clone())) {
        th.err
    } else if tab.active {
        th.muted
    } else {
        th.dim
    };
    [
        PaneTab {
            spans: label,
            hit: Some(HitTarget::LauncherTab(tab.id.clone())),
        },
        PaneTab {
            spans: vec![Span::styled(" × ", fill(Style::default().fg(cross)))],
            hit: Some(HitTarget::LauncherTabClose(tab.id.clone())),
        },
    ]
}

/// A PROJECT TAB's STATUS DOTS: one per state its sessions are in, each
/// carrying that state's count and no word at all, so the header is read
/// at a glance rather than parsed. Waiting on you leads (red), then
/// finished unread (blue), then working (yellow) — the three a row's own
/// STATUS DOT wears. A state with nothing in it is left out, so a quiet
/// project is its bare name — and because the order is fixed, the dots
/// that are there never move as the work under them does.
fn tab_dots(tally: Tally, th: Theme) -> Vec<Span<'static>> {
    [
        (tally.needs_you, th.err),
        (tally.done, th.done),
        (tally.running, th.warn),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, color)| Span::styled(format!(" ●{n}"), Style::default().fg(color)))
    .collect()
}

/// The ramp a PROJECT TAB's name sweeps on: red while any of its sessions
/// waits on you, whatever else is going on; yellow while any is mid-turn;
/// and blue once none is, for as long as a finish is left unread — the
/// project's work is done and you have not looked. A quiet project holds
/// still. Unlike a card's ONE-SHOT SWEEP the blue lasts, since the tab is
/// how a finish in a project you are not on gets noticed at all.
fn tab_ramp(tally: Tally, th: Theme) -> Option<[Color; 3]> {
    if tally.needs_you > 0 {
        Some(th.err_sweep)
    } else if tally.running > 0 {
        Some(th.warn_sweep)
    } else if tally.done > 0 {
        Some(th.done_sweep)
    } else {
        None
    }
}

/// The header's right side: how many cards the grid holds, and how many
/// of them are off screen. Which of them want something is the STATUS
/// DOTS' business on the PROJECT TABS, told in dots rather than in a
/// second sentence.
///
/// It gets what the tabs leave of the row ([`draw_head`]), and what does
/// not fit in `width` gives way whole, least needed first: the count of
/// the cards (each band's rule says its own), then the PR & ISSUE COUNTS
/// (`v` and `i` say them again), and the HIDDEN MARKER last — a
/// screenful of cards with more behind it looks exactly like a project
/// with that many sessions in it, and the PANE dragged up over the grid
/// is the usual way of getting there. The row never runs past `width`.
///
/// Each span comes with the button it is, if any: only the two counts are,
/// and [`draw_head`] lays their hit rects where the row lands them.
fn head_count(
    app: &App,
    count: HeadCount,
    hidden: Hidden,
    width: usize,
    th: Theme,
) -> Vec<(Span<'static>, Option<HitTarget>)> {
    type Part = Vec<(Span<'static>, Option<HitTarget>)>;
    let words: Part = vec![(
        Span::styled(count_words(app, count), Style::default().fg(th.dim)),
        None,
    )];
    // The PR & ISSUE COUNTS the PROJECTS PANEL's rows used to carry: how
    // many open pull requests and issues the project in front of you has,
    // so the number is read without opening `v` or `i` to find it — and a
    // click on either count opens that list, the pointer's way to the
    // same modal. A second column of air sets them off from the count.
    let badge: Part = app
        .selected_project()
        .and_then(|project| crate::ui::open_counts_badge(app.project_open_counts(&project.id), th))
        .map(|(parts, _)| {
            let parts = parts.into_iter().map(|(text, mut style, hit)| {
                // Nothing about a word says it is a button, so the one
                // under the pointer is underlined, as the header's tabs
                // are.
                if hit.is_some() && app.launcher.hover_crumb == hit {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                (Span::styled(text, style), hit)
            });
            std::iter::once((Span::raw(" "), None))
                .chain(parts)
                .collect()
        })
        .unwrap_or_default();
    let mark: Part = hidden_mark(hidden, th)
        .into_iter()
        .map(|s| (s, None))
        .collect();
    let underline = |hit: &HitTarget, style: Style| {
        if app.launcher.hover_crumb.as_ref() == Some(hit) {
            style.add_modifier(Modifier::UNDERLINED)
        } else {
            style
        }
    };
    let toggle: Part = view_toggle(app.launcher.launcher_kanban, th)
        .into_iter()
        .map(|(span, hit)| match hit {
            Some(hit) => (
                Span::styled(span.content, underline(&hit, span.style)),
                Some(hit),
            ),
            None => (span, None),
        })
        .collect();
    let scope: Part = scope_chip(app, th);
    // Once a piece has not fitted, nothing after it is tried: a narrower
    // piece slipping into the room a wider one left would swap what the
    // row says as the window narrows, rather than only take it away.
    let mut left = Some(width);
    let mut keep = |part: Part| -> Part {
        let w: usize = part.iter().map(|(s, _)| s.width()).sum();
        match left {
            Some(room) if w <= room => {
                left = Some(room - w);
                part
            }
            _ => {
                left = None;
                Vec::new()
            }
        }
    };
    // The HEADER TOOLS at the row's right end: Settings and Help, the two
    // things every screen of kete has and a pointer had no way to. Lit
    // under the pointer as the toggle is; the footer names their keys
    // while it rests there.
    let tools: Part = std::iter::once((Span::raw(" "), None))
        .chain(
            [
                (HEAD_SETTINGS, crate::keymap::Action::Settings),
                (HEAD_HELP, crate::keymap::Action::Help),
            ]
            .into_iter()
            .map(|(glyph, action)| {
                let hit = HitTarget::Button(action);
                let style = if app.launcher.hover_crumb.as_ref() == Some(&hit) {
                    Style::default().fg(th.accent)
                } else {
                    Style::default().fg(th.muted)
                };
                (Span::styled(glyph, style), Some(hit))
            }),
        )
        .collect();
    // The VIEW TOGGLE is kept first, so it is the last thing a narrow
    // row gives up: it is the one way to the board a pointer can see.
    // The tools after everything that says something about the grid:
    // the footer offers both keys as buttons too.
    let mut toggle = keep(toggle);
    let scope = keep(scope);
    let mark = keep(mark);
    let badge = keep(badge);
    let words = keep(words);
    let tools = keep(tools);
    // The toggle's key last of all, in what the rest left: a narrow row
    // keeps its counts over the reminder of a key the footer also names.
    let key = Span::styled(
        format!(
            " {}",
            crate::ui::key_hint(app, crate::keymap::Action::ToggleKanban)
        ),
        Style::default().fg(th.dim),
    );
    if !toggle.is_empty() && left.is_some_and(|room| key.width() <= room) {
        toggle.push((key, None));
    }
    words
        .into_iter()
        .chain(scope)
        .chain(badge)
        .chain(mark)
        .chain(toggle)
        .chain(tools)
        .collect()
}

/// The SCOPE CHIP: what the bench is looking at, `all ▸ repo ▸ worktree`
/// (and the account, when the scope has one), each segment a button that
/// widens the scope back to it and a `✕` that clears it — the board, the
/// ATTENTION STRIP, `.`/`,` and the quick prompt all follow it. Drawn on
/// the board always, and on the grid while something is narrowed. FOCUS
/// HERE (the menus, `?`) narrows it; Esc, once the cursor has let go of
/// the card, clears it as the `✕` does.
fn scope_chip(app: &App, th: Theme) -> Vec<(Span<'static>, Option<HitTarget>)> {
    let segments = crate::launcher::scope_segments(app);
    if segments.is_empty() && !app.launcher.launcher_kanban {
        return Vec::new();
    }
    let lit = |hit: &HitTarget, style: Style| {
        if app.launcher.hover_crumb.as_ref() == Some(hit) {
            style.add_modifier(Modifier::UNDERLINED)
        } else {
            style
        }
    };
    let mut out = vec![(Span::raw(" · "), None)];
    let all = HitTarget::ScopeChip(0);
    let all_style = if segments.is_empty() {
        Style::default().fg(th.muted)
    } else {
        Style::default().fg(th.dim)
    };
    out.push((Span::styled("all", lit(&all, all_style)), Some(all)));
    let last = segments.len();
    let named = app.launcher.scope.account.is_some();
    for (i, (word, keep)) in segments.into_iter().enumerate() {
        out.push((Span::styled(" ▸ ", Style::default().fg(th.dim)), None));
        // The account's segment opens its picker; the rest widen to
        // themselves.
        let hit = if keep == 4 {
            HitTarget::ScopeAccount
        } else {
            HitTarget::ScopeChip(keep)
        };
        let style = if i + 1 == last {
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        out.push((
            Span::styled(truncate(&word, 18), lit(&hit, style)),
            Some(hit),
        ));
    }
    // No account in the scope: a bare `@` picks one.
    if !named {
        let hit = HitTarget::ScopeAccount;
        out.push((
            Span::styled(" @", lit(&hit, Style::default().fg(th.dim))),
            Some(hit),
        ));
    }
    if last > 0 {
        let hit = HitTarget::ScopeClear;
        out.push((
            Span::styled(" ✕", lit(&hit, Style::default().fg(th.muted))),
            Some(hit),
        ));
    }
    out
}

/// The HEADER TOOLS' buttons ([`head_count`]): Settings, then Help.
const HEAD_SETTINGS: &str = " ⚙ ";
const HEAD_HELP: &str = " ? ";

/// The header's VIEW TOGGLE, the desktop's Overview|Board radio: the two
/// homes side by side, the one on screen lit as a solid chip, the other a
/// button ([`HitTarget::LauncherHome`]). [`head_count`] adds the key that
/// flips them where there is room.
fn view_toggle(board: bool, th: Theme) -> Vec<(Span<'static>, Option<HitTarget>)> {
    let half = |label: &'static str, on: bool, hit: bool| {
        let style = if on {
            // The lit PROJECT TAB's own chip.
            Style::default()
                .fg(th.accent)
                .bg(th.sel_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        (
            Span::styled(format!(" {label} "), style),
            Some(HitTarget::LauncherHome(hit)),
        )
    };
    vec![
        (Span::raw("  "), None),
        half("Overview", !board, false),
        (Span::styled("│", Style::default().fg(th.edge)), None),
        half("Board", board, true),
    ]
}

/// The header's count of what the grid holds: `3 sessions · 2 terminals`
/// — the terminals only when there are any, since most projects have
/// none — under the ARCHIVED VIEW's own word there, so the header says
/// which of the two lists is on screen without a second line to read.
fn count_words(app: &App, count: HeadCount) -> String {
    let noun = if app.launcher.show_archived {
        "archived session"
    } else {
        "session"
    };
    let mut out = format!("{} {noun}{}", count.sessions, plural(count.sessions));
    if count.terminals > 0 {
        out.push_str(&format!(
            " · {} terminal{}",
            count.terminals,
            plural(count.terminals)
        ));
    }
    out
}

/// The HIDDEN MARKER: how many bands — or, inside a worktree, cards — the
/// grid holds that are not on screen, with an arrow saying which way
/// they went — `↓` past the bottom edge, `↑` scrolled off the top, `↑↓`
/// both. Nothing at all when the window holds the lot, so a grid that
/// fits reads exactly as it does today.
///
/// This is the row's answer to a PANE dragged up over the cards: the
/// count beside it still says how many sessions the project has, and this
/// says how many of them the room left can show — so a card that went
/// missing reads as the pane taking its room rather than as the session
/// disappearing, and the arrow says whether the way back to it is a step
/// down through the grid or the pane's own edge dragged back.
fn hidden_mark(hidden: Hidden, th: Theme) -> Vec<Span<'static>> {
    let total = hidden.total();
    if total == 0 {
        return Vec::new();
    }
    let arrow = match (hidden.above > 0, hidden.below > 0) {
        (true, true) => "↑↓",
        (true, false) => "↑",
        _ => "↓",
    };
    vec![Span::styled(
        format!("  {arrow} {total} hidden"),
        Style::default().fg(th.muted),
    )]
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The rule under a header row.
fn draw_rule(f: &mut Frame, area: Rect, row: usize, color: Color) {
    if let Some(r) = row_rect(area, row) {
        f.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(r.width as usize),
                Style::default().fg(color),
            )),
            r,
        );
    }
}

/// The header's own margin, so its text lines up with the cards' left
/// edge rather than hugging the screen.
fn pad_x(r: Rect) -> Rect {
    let pad = crate::launcher::PAD_X;
    Rect {
        x: r.x + pad,
        width: r.width.saturating_sub(pad * 2),
        ..r
    }
}

/// The GRID: one band per checkout, top to bottom — a collapsed one its
/// rule and one row of cards, the ACCORDION's open one its rule and
/// every card wrapped into rows under it — the whole panel scrolled by
/// rows so the cursor's card is always drawn, a band that straddles the
/// window's edge drawn cut ([`draw_cut`]) rather than left out.
fn draw_bands(
    f: &mut Frame,
    app: &mut App,
    g: &crate::launcher::BandsLayout,
    panel: &crate::launcher::PanelLayout,
    bands: &[crate::launcher::Band],
    cursor: Option<usize>,
    scroll: u16,
) {
    let th = app.chrome.theme;
    let window = panel.window();
    // One CONFIG.JSON read for the whole frame, and only if some card on
    // it runs a CUSTOM harness whose label lives in there — a screenful
    // of cards must not reload the file once per card.
    let mut cfg = None;
    // The keys are up on the PROJECT TABS, or down in the pane: the band
    // under the cursor keeps its bold branch — it still says which
    // checkout the pane reads — but the accent goes with the keys, so
    // only the one thing holding them is lit.
    let keys = app.nav.focus != Focus::Terminal && app.launcher.launcher_tab_cursor.is_none();
    // The band the selection is on, aimed at or let go of: its row
    // follows its remembered card either way, so Esc scrolls nothing.
    let aimed = crate::launcher::band_cursor(app, bands);
    // Each band's whole rectangle, rule to last row of cards: a click on
    // the air around its cards picks the band as a click on its rule
    // does. Pushed after every card, rule and arrow so they keep their
    // own targets under `hit_at`'s first-match scan.
    let mut band_areas = Vec::with_capacity(bands.len());
    for (index, band) in bands.iter().enumerate() {
        let pb = &panel.bands[index];
        let whole = Rect {
            y: pb.rule_y,
            height: pb.height,
            ..g.area
        };
        if let Some(placed) = crate::launcher::place(window, scroll, whole) {
            band_areas.push((placed.rect, HitTarget::LauncherBand(index)));
        }
        let on = cursor == Some(index);
        let at = (aimed == Some(index))
            .then(|| crate::launcher::card_cursor(app, band))
            .flatten();
        // Collapsed: the STRIP's one row of cards under the rule, its
        // `y` wherever the panel put the band.
        let strip = pb.content.is_none().then(|| {
            g.strip_at(
                Rect {
                    y: pb.rule_y,
                    height: crate::launcher::BAND_H,
                    ..g.area
                },
                index,
                band,
                at,
            )
        });
        let rule = Rect {
            y: pb.rule_y,
            height: crate::launcher::BAND_RULE_H,
            ..g.area
        };
        // A one-row rule is on screen whole or not at all
        // ([`crate::launcher::place`]), so it needs no [`draw_cut`].
        if let Some(placed) = crate::launcher::place(window, scroll, rule) {
            let hits = draw_band_rule(
                f.buffer_mut(),
                app,
                placed.rect,
                band,
                BandRule {
                    index,
                    on,
                    lit: on && keys,
                    more: match &strip {
                        Some(strip) => strip.hidden(),
                        None => pb.content.as_ref().map_or(0, |c| c.more),
                    },
                    expanded: pb.open,
                    list: app.launcher.launcher_list,
                },
            );
            app.chrome.hits.extend(hits);
        }
        if band.cards.is_empty() {
            draw_empty_band(f, app, g, pb, band, (window, scroll), on && keys);
            continue;
        }
        // The cards under the rule: on the band the cursor is on, the
        // one it remembers — what the pane reads, and what the keys walk
        // — is raised out of the row ([`selected_card_block`]), its fill
        // brighter while the keys are on the grid; the rest are a preview. A click on any lands the
        // cursor on it (`HitTarget::LauncherCard`), and a card drawn cut
        // is clicked on the rows of it there are: the landing scrolls
        // the rest of it into view (`settle_panel_scroll`).
        if app.launcher.launcher_list {
            draw_list_band(
                f,
                app,
                g,
                pb,
                ListBand {
                    index,
                    band,
                    window,
                    scroll,
                    on,
                    at,
                    lit: on && keys,
                },
                &mut cfg,
            );
            continue;
        }
        match &strip {
            None => {
                // Open: every card, wrapped into rows, the terminals
                // under a section rule of their own. The sessions' rule
                // is the band's, drawn above.
                for (y, label) in pb.rules() {
                    if label == crate::launcher::SESSIONS_RULE {
                        continue;
                    }
                    let rule = Rect {
                        y,
                        height: crate::launcher::BAND_RULE_H,
                        ..g.area
                    };
                    let Some(placed) = crate::launcher::place(window, scroll, rule) else {
                        continue;
                    };
                    let fill = "─".repeat(
                        usize::from(placed.rect.width).saturating_sub(label.chars().count() + 4),
                    );
                    f.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled("── ", Style::default().fg(th.edge)),
                            Span::styled(label.to_string(), Style::default().fg(th.dim)),
                            Span::styled(format!(" {fill}"), Style::default().fg(th.edge)),
                        ])),
                        placed.rect,
                    );
                }
                for (i, card) in band.cards.iter().enumerate() {
                    let Some(placed) = pb
                        .cell(i)
                        .and_then(|cell| crate::launcher::place(window, scroll, cell))
                    else {
                        continue;
                    };
                    let selected = on && at == Some(i);
                    draw_cut(f, placed, |buf, r| {
                        draw_any_card(buf, &*app, r, card, selected, keys, th, &mut cfg)
                    });
                    let at = crate::launcher::CardRef {
                        band: index,
                        card: i,
                    };
                    selected_card_buttons(f, app, card, at, placed, selected);
                    note_tail_card(app, card);
                    app.chrome.hits.extend(card_issue_hit(app, card, placed));
                    app.chrome
                        .hits
                        .extend(super::subagents_view::card_chip_hit(app, card, placed));
                    app.chrome
                        .hits
                        .push((placed.rect, HitTarget::LauncherCard(at)));
                }
            }
            Some(strip) => {
                let row = Rect {
                    y: pb.rule_y + crate::launcher::BAND_RULE_H,
                    height: crate::launcher::CARD_H,
                    ..g.area
                };
                let Some(row_placed) = crate::launcher::place(window, scroll, row) else {
                    continue;
                };
                for slot in &strip.cards {
                    let placed = crate::launcher::Placed {
                        rect: Rect {
                            x: slot.rect.x,
                            width: slot.rect.width,
                            ..row_placed.rect
                        },
                        cut_top: row_placed.cut_top,
                        cut_bottom: row_placed.cut_bottom,
                    };
                    let card = &band.cards[slot.at.card];
                    let selected = on && at == Some(slot.at.card);
                    draw_cut(f, placed, |buf, r| {
                        draw_any_card(buf, &*app, r, card, selected, keys, th, &mut cfg)
                    });
                    selected_card_buttons(f, app, card, slot.at, placed, selected);
                    note_tail_card(app, card);
                    app.chrome.hits.extend(card_issue_hit(app, card, placed));
                    app.chrome
                        .hits
                        .extend(super::subagents_view::card_chip_hit(app, card, placed));
                    app.chrome
                        .hits
                        .push((placed.rect, HitTarget::LauncherCard(slot.at)));
                }
                // The arrows only on a row drawn whole: they stand beside
                // the cards' full height.
                if row_placed.whole() {
                    draw_strip_arrows(f, app, strip, row_placed.rect.y, index, on && keys, th);
                }
                if strip.hidden() > 0 {
                    // The band's own row under the cards
                    // ([`crate::launcher::MORE_H`]), the gap to the next
                    // band's rule under it.
                    let row = Rect {
                        y: pb.rule_y + crate::launcher::BAND_H,
                        height: crate::launcher::MORE_H,
                        ..g.area
                    };
                    if let Some(placed) = crate::launcher::place(window, scroll, row) {
                        let more = StripMore {
                            index,
                            hidden: strip.hidden(),
                            lit: on && keys,
                            centered: true,
                        };
                        draw_strip_more(f, app, placed.rect, band, more);
                    }
                }
            }
        }
    }
    draw_panel_edge_marks(f, panel, scroll, th);
    app.chrome.hits.extend(band_areas);
    // Last, so the bands themselves win `hit_at`'s first-match scan and
    // only the air between them falls through to the grid.
    app.chrome
        .hits
        .push((g.area, HitTarget::PanelBg(Focus::Sessions)));
}

/// What an EMPTY BAND says under its rule, the user's own keys named: the
/// root's without the delete it refuses.
pub(crate) fn empty_band_hint(app: &App, is_main: bool) -> String {
    let k = |a| crate::ui::key_hint(app, a);
    let mut text = format!(
        "nothing running · {}: new session · {}: terminal",
        k(crate::keymap::Action::QuickPrompt),
        k(crate::keymap::Action::NewTerminal)
    );
    if !is_main {
        text.push_str(&format!(
            " · {}: delete worktree",
            k(crate::keymap::Action::Delete)
        ));
    }
    text
}

/// The line under an EMPTY BAND's rule — a checkout with nothing running
/// in it, drawn only with **Show all worktrees** on — in place of the row
/// of cards it has none of: what can be done there, dim, and in the
/// accent while the cursor is on the band with the keys on the grid.
/// A click on it is a click on the band (the band's whole area).
fn draw_empty_band(
    f: &mut Frame,
    app: &App,
    g: &crate::launcher::BandsLayout,
    pb: &crate::launcher::PanelBand,
    band: &crate::launcher::Band,
    (window, scroll): (Rect, u16),
    lit: bool,
) {
    let th = app.chrome.theme;
    let row = Rect {
        y: pb.rule_y + crate::launcher::BAND_RULE_H,
        height: crate::launcher::EMPTY_BAND_ROW_H,
        ..g.area
    };
    let Some(placed) = crate::launcher::place(window, scroll, row) else {
        return;
    };
    let text = empty_band_hint(app, band.is_main);
    let fg = if lit { th.accent } else { th.dim };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {text}"),
            Style::default().fg(fg),
        ))),
        placed.rect,
    );
}

/// The EDGE MARKERS on a grid taller than its window: `↑ 2 more above`
/// on the header's row of air over the cards, `↓ 3 more below` on the
/// row kept under them ([`crate::launcher::PanelLayout::window`]) —
/// each where the eye looks for the rest, so the grid says it scrolls,
/// and which way, without a word painted over a card. With every band
/// collapsed what is past the edge is whole checkouts, and the marker
/// says so (`↓ 3 more worktrees below`), so a screenful that happens to
/// end on a band does not read as the whole project; with one open its
/// cards are in the count too, and the marker just counts. The header's
/// `↑↓ 5 hidden` says the same once more. A grid that fits has neither
/// row marked, and a panel scrolled to an end leaves that end's row as
/// plain air.
fn draw_panel_edge_marks(
    f: &mut Frame,
    panel: &crate::launcher::PanelLayout,
    scroll: u16,
    th: Theme,
) {
    if !panel.overflows() {
        return;
    }
    let hidden = panel.hidden(scroll);
    let what = |n: usize| {
        if panel.open().is_some() {
            String::new()
        } else {
            format!(" worktree{}", plural(n))
        }
    };
    let mark = |f: &mut Frame, r: Rect, words: String| {
        f.render_widget(
            Paragraph::new(Span::styled(words, Style::default().fg(th.muted)))
                .alignment(ratatui::layout::Alignment::Center),
            r,
        );
    };
    if hidden.above > 0 {
        let r = Rect {
            y: panel.area.y.saturating_sub(1),
            height: 1,
            ..panel.area
        };
        let n = hidden.above;
        mark(f, r, format!("↑ {n} more{} above", what(n)));
    }
    if hidden.below > 0 {
        let window = panel.window();
        let r = Rect {
            y: window.y + window.height,
            height: crate::launcher::BELOW_MARK_H,
            ..panel.area
        };
        let n = hidden.below;
        mark(f, r, format!("↓ {n} more{} below", what(n)));
    }
}

/// The `❮` and `❯` beside a band's row.
const STRIP_LEFT: &str = "❮";
const STRIP_RIGHT: &str = "❯";

/// What stands beside a band's row when it holds more cards than it
/// shows: `❮` in the margin before its first card for the ones scrolled
/// past the left edge, `❯` after its last card for the ones past the
/// right, each on the row's middle line — in the accent while the band
/// holds the keys, muted on any other band, so a glance says the row
/// goes on without lighting it up. Each is a button: its hit is the
/// two-cell gap the glyph stands in, the cards' full height, and a click
/// on it is the step `h` / `l` take that way
/// (`HitTarget::LauncherStripLeft`, `LauncherStripRight`). Under the
/// pointer the glyph takes the accent whichever band it is on.
fn draw_strip_arrows(
    f: &mut Frame,
    app: &mut App,
    strip: &crate::launcher::Strip,
    top: u16,
    index: usize,
    lit: bool,
    th: Theme,
) {
    use crate::launcher::{CARD_H, PAD_X};
    let (Some(first), Some(last)) = (strip.cards.first(), strip.cards.last()) else {
        return;
    };
    let frame = f.area();
    // `top` is the row's line on screen: the slots' own `y` counts from
    // the top of the whole panel, before the window and its scroll.
    let y = top + CARD_H / 2;
    let mut arrow = |hit: HitTarget, x: u16, glyph_x: u16, glyph: &str| {
        let hovered = app.launcher.hover_crumb.as_ref() == Some(&hit);
        let style = if lit || hovered {
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        let cell = Rect {
            x: glyph_x,
            y,
            width: 1,
            height: 1,
        }
        .intersection(frame);
        if cell.width > 0 {
            f.render_widget(Paragraph::new(Span::styled(glyph.to_string(), style)), cell);
        }
        let target = Rect {
            x,
            y: top,
            width: PAD_X,
            height: CARD_H,
        }
        .intersection(frame);
        if target.width > 0 {
            app.chrome.hits.push((target, hit));
        }
    };
    if strip.before > 0 {
        // The margin before the row: the glyph, then a cell of air
        // against the first card.
        let x = first.rect.x.saturating_sub(PAD_X);
        arrow(HitTarget::LauncherStripLeft(index), x, x, STRIP_LEFT);
    }
    if strip.after > 0 {
        // The gap after the last card drawn — between cards or the
        // margin, whichever the row ends in: a cell of air, then the
        // glyph.
        let x = last.rect.x + last.rect.width;
        arrow(HitTarget::LauncherStripRight(index), x, x + 1, STRIP_RIGHT);
    }
}

/// The MORE HINT under a collapsed band's row that left cards off its
/// edges: `▾ 6 more · Tab: see all 8`, centered on the band's own row
/// under the cards ([`crate::launcher::MORE_H`]), so a band hiding sessions says so where the eye leaves the
/// cards rather than only at the far end of its rule. Muted, its key in
/// the accent while the band holds the keys or the pointer is on it. A
/// button: a click opens the band as the ACCORDION, the very toggle Tab
/// runs (`HitTarget::LauncherBandMore`).
fn draw_strip_more(
    f: &mut Frame,
    app: &mut App,
    r: Rect,
    band: &crate::launcher::Band,
    more: StripMore,
) {
    let StripMore {
        index,
        hidden,
        lit,
        centered,
    } = more;
    let th = app.chrome.theme;
    let hit = HitTarget::LauncherBandMore(index);
    let hovered = app.launcher.hover_crumb.as_ref() == Some(&hit);
    let key = super::key_hint(app, crate::keymap::Action::FocusNext);
    let (words, does) = (
        format!("▾ {hidden} more · "),
        format!(": see all {}", band.cards.len()),
    );
    let key_style = if lit || hovered {
        Style::default().fg(th.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.dim)
    };
    let text = Style::default().fg(if hovered { th.accent } else { th.muted });
    let line = Line::from(vec![
        Span::styled(words, text),
        Span::styled(key, key_style),
        Span::styled(does, text),
    ]);
    let w = (line.width() as u16).min(r.width);
    // Centered under a row of cards; under a LIST, in the column its
    // entries' names start in.
    let x = if centered {
        r.x + (r.width - w) / 2
    } else {
        r.x + LIST_LEAD.min(r.width - w)
    };
    let at = Rect { x, width: w, ..r };
    f.render_widget(Paragraph::new(line), at);
    app.chrome.hits.push((at, hit));
}

/// What [`draw_strip_more`] says, and where: the band it is under, how
/// many it counts, whether the band holds the keys, and whether it
/// centers under a row of cards or starts where a LIST's names do.
#[derive(Debug, Clone, Copy)]
struct StripMore {
    index: usize,
    hidden: usize,
    lit: bool,
    centered: bool,
}

/// What the cursor's LIST entry wears in front of it.
const LIST_MARK: &str = "▌ ";
/// Columns a LIST entry spends before its name: the cursor mark and the
/// status dot (or a terminal's glyph), two apiece.
const LIST_LEAD: u16 = 4;
/// Longest a LIST's name column grows, and its runs-on column: past these
/// the entries truncate rather than push the prompt off the line.
const LIST_NAME_MAX: usize = 28;
const LIST_RUNS_MAX: usize = 24;
/// Air between a LIST entry's columns.
const LIST_GAP: usize = 2;

/// One band of the compact LIST as [`draw_list_band`] draws it.
struct ListBand<'a> {
    index: usize,
    band: &'a crate::launcher::Band,
    window: Rect,
    scroll: u16,
    /// The cursor is on the band, and `at` is its card.
    on: bool,
    at: Option<usize>,
    /// The keys are on the band too.
    lit: bool,
}

/// A band in the compact LIST: under its rule, one line per entry the
/// layout shows — its most recent sessions, or every card once it is
/// open (`launcher::list_layout`) — the names and what each runs on in
/// columns shared down the band, then the MORE HINT on the line under
/// them when it leaves any off. A click on a line lands the cursor on its
/// card (`HitTarget::LauncherCard`), exactly as a click on a card does.
fn draw_list_band(
    f: &mut Frame,
    app: &mut App,
    g: &crate::launcher::BandsLayout,
    pb: &crate::launcher::PanelBand,
    list: ListBand<'_>,
    cfg: &mut Option<crate::config::Config>,
) {
    let ListBand {
        index,
        band,
        window,
        scroll,
        on,
        at,
        lit,
    } = list;
    let Some(content) = pb.content.as_ref() else {
        return;
    };
    let shown: Vec<usize> = content
        .rows
        .iter()
        .filter_map(|r| r.first().copied())
        .collect();
    let mut cols = (0, 0);
    for &i in &shown {
        let (name, runs) = match &band.cards[i] {
            crate::launcher::Card::Session(row) => (
                row.agent.name.chars().count(),
                runs_on_line(&row.agent, cfg).chars().count(),
            ),
            crate::launcher::Card::Terminal(t) => (
                t.name.chars().count(),
                t.run_command.as_deref().unwrap_or("shell").chars().count(),
            ),
        };
        cols = (cols.0.max(name), cols.1.max(runs));
    }
    let cols = (cols.0.min(LIST_NAME_MAX), cols.1.min(LIST_RUNS_MAX));
    for &i in &shown {
        let Some(placed) = pb
            .cell(i)
            .and_then(|cell| crate::launcher::place(window, scroll, cell))
        else {
            continue;
        };
        let card = &band.cards[i];
        let selected = on && at == Some(i);
        draw_list_row(
            f.buffer_mut(),
            app,
            placed.rect,
            card,
            (selected, selected && lit),
            cols,
            cfg,
        );
        note_tail_card(app, card);
        app.chrome.hits.push((
            placed.rect,
            HitTarget::LauncherCard(crate::launcher::CardRef {
                band: index,
                card: i,
            }),
        ));
    }
    if let Some(y) = content.more_y() {
        let row = Rect {
            y: pb.rule_y + y,
            height: crate::launcher::MORE_H,
            ..g.area
        };
        if let Some(placed) = crate::launcher::place(window, scroll, row) {
            let more = StripMore {
                index,
                hidden: content.more,
                lit,
                centered: false,
            };
            draw_strip_more(f, app, placed.rect, band, more);
        }
    }
}

/// One entry of the compact LIST, on one line: the cursor mark, the
/// status dot and name a card heads with, what it runs on, and the last
/// thing it was asked — `›` as on the card — or, for a terminal, the last
/// line its shell printed; how long since it moved (or `exited`) at the
/// right. `cols` are the band's name and runs-on column widths, so the
/// entries line up. `(selected, lit)`: the cursor's entry wears the mark,
/// ([`LIST_MARK`]), and the FOCUSED PANEL TINT across the line while the
/// grid holds the keys.
fn draw_list_row(
    buf: &mut Buffer,
    app: &App,
    r: Rect,
    card: &crate::launcher::Card,
    (selected, lit): (bool, bool),
    (name_col, runs_col): (usize, usize),
    cfg: &mut Option<crate::config::Config>,
) {
    let th = app.chrome.theme;
    let width = usize::from(r.width);
    // A bar rather than the rule's `❯`: a shell's own glyph is `❯`, one
    // column over, and the two would read as one mark.
    let mark = if selected {
        Span::styled(LIST_MARK, Style::default().fg(th.accent))
    } else {
        Span::raw("  ")
    };
    struct Entry {
        lead: Span<'static>,
        name: String,
        name_style: Style,
        ramp: Option<[Color; 3]>,
        runs: String,
        runs_style: Style,
        text_mark: &'static str,
        text: String,
        text_style: Style,
        badge: String,
        badge_style: Style,
    }
    let e = match card {
        crate::launcher::Card::Session(row) => {
            let a = &row.agent;
            let look = session_look(app, a, selected, th);
            let quiet_or = |live: Color| if a.archived { look.quiet } else { live };
            Entry {
                lead: look.dot,
                name: a.name.clone(),
                name_style: look.name_style,
                ramp: look.ramp,
                runs: runs_on_line(a, cfg),
                runs_style: Style::default().fg(quiet_or(th.dim)),
                text_mark: "› ",
                text: crate::launcher::last_prompt(a)
                    .unwrap_or_default()
                    .to_string(),
                text_style: Style::default().fg(quiet_or(th.muted)),
                badge: look.ago,
                badge_style: look.ago_style,
            }
        }
        crate::launcher::Card::Terminal(t) => Entry {
            lead: Span::styled(
                if t.run_command.is_some() {
                    "▶ "
                } else {
                    "❯ "
                },
                Style::default().fg(if t.alive { th.ok } else { th.dim }),
            ),
            name: t.name.clone(),
            name_style: Style::default().fg(th.text).add_modifier(Modifier::BOLD),
            ramp: None,
            runs: t.run_command.clone().unwrap_or_else(|| "shell".into()),
            runs_style: Style::default().fg(th.dim),
            text_mark: "",
            text: terminal_tail_lines(app, t)
                .iter()
                .map(crate::terminal_tail::TailRow::text)
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default(),
            text_style: Style::default().fg(th.muted),
            badge: if t.alive {
                String::new()
            } else {
                EXITED_BADGE.into()
            },
            badge_style: Style::default().fg(th.err),
        },
    };
    let mut spans = vec![mark, e.lead];
    let mut room = width.saturating_sub(usize::from(LIST_LEAD));
    // The badge at the right end, while the name keeps a few letters.
    let badge = e.badge.trim().to_string();
    let badge_w = badge.chars().count();
    let keep_badge = badge_w > 0 && room >= badge_w + 1 + 8;
    if keep_badge {
        room -= badge_w + 1;
    }
    let name_w = name_col.max(1).min(room);
    let name = truncate(&e.name, name_w);
    let mut used = name.chars().count();
    spans.extend(status_name_spans(
        name,
        e.name_style,
        e.ramp,
        app.sweep_phase(),
    ));
    // What it runs on, in the band's column, and the prompt after it —
    // each only where there is room for a few letters of it.
    let runs_at = name_w + LIST_GAP;
    if !e.runs.is_empty() && room >= runs_at + 6 {
        let runs_w = runs_col.min(room - runs_at);
        let runs = truncate(&e.runs, runs_w);
        spans.push(Span::raw(" ".repeat(runs_at - used)));
        used = runs_at + runs.chars().count();
        spans.push(Span::styled(runs, e.runs_style));
        let text_at = runs_at + runs_w + LIST_GAP;
        let text_mark_w = e.text_mark.chars().count();
        if !e.text.is_empty() && room >= text_at + text_mark_w + 6 {
            let text = truncate(&e.text, room - text_at - text_mark_w);
            spans.push(Span::raw(" ".repeat(text_at - used)));
            spans.push(Span::styled(e.text_mark, Style::default().fg(th.dim)));
            used = text_at + text_mark_w + text.chars().count();
            spans.push(Span::styled(text, e.text_style));
        }
    }
    if keep_badge {
        spans.push(Span::raw(" ".repeat(room.saturating_sub(used) + 1)));
        spans.push(Span::styled(badge, e.badge_style));
    }
    let mut line = Paragraph::new(Line::from(spans));
    if lit {
        line = line.style(Style::default().bg(th.focus_tint));
    }
    line.render(r, buf);
}

/// Draw something the window's edges may cut — a card half scrolled off
/// the top — as the terminal draws a line half scrolled off: the rows of
/// it inside the window, and only those. `draw` paints the whole thing
/// into the rect it is handed; one drawn whole paints straight onto the
/// frame, one that is cut paints onto a scratch buffer its own size and
/// the rows on screen are copied across. So a card's top border is
/// never drawn on a card whose top is off screen — a whole-looking card
/// where a cut one stands would say the wrong thing about what is
/// above it.
fn draw_cut(f: &mut Frame, placed: crate::launcher::Placed, draw: impl FnOnce(&mut Buffer, Rect)) {
    if placed.whole() {
        draw(f.buffer_mut(), placed.rect);
        return;
    }
    let full = Rect {
        y: 0,
        height: placed.rect.height + placed.cut_top + placed.cut_bottom,
        ..placed.rect
    };
    let mut scratch = Buffer::empty(full);
    draw(&mut scratch, full);
    let buf = f.buffer_mut();
    for dy in 0..placed.rect.height {
        for x in placed.rect.left()..placed.rect.right() {
            let Some(from) = scratch.cell((x, placed.cut_top + dy)) else {
                continue;
            };
            if let Some(to) = buf.cell_mut((x, placed.rect.y + dy)) {
                *to = from.clone();
            }
        }
    }
}

/// A card of either kind, in `area`.
#[allow(clippy::too_many_arguments)]
fn draw_any_card(
    buf: &mut Buffer,
    app: &App,
    area: Rect,
    card: &crate::launcher::Card,
    selected: bool,
    focused: bool,
    th: Theme,
    cfg: &mut Option<crate::config::Config>,
) {
    match card {
        crate::launcher::Card::Session(row) => {
            draw_card(buf, app, area, row, selected, focused, th, cfg);
            super::subagents_view::draw_card_chip(buf, app, area, &row.agent, th);
        }
        crate::launcher::Card::Terminal(t) => draw_chip(buf, app, area, t, selected, focused, th),
    }
}

/// A terminal card just drawn: the grid's beat asks the daemon what its
/// shell printed since ([`App::tail_cards`], `event_loop::request_terminal_tails`).
fn note_tail_card(app: &mut App, card: &crate::launcher::Card) {
    if let crate::launcher::Card::Terminal(t) = card {
        app.pane.tail_cards.push(t.id.clone());
    }
}

/// The least room a band's rule keeps for its glyph and branch before
/// the Enter hint at its right end gives way: the mark and the counts
/// stay, the words go.
const ENTER_HINT_ROOM: usize = 12;

/// Where a band's rule is being drawn: always on the grid over its
/// cards — the ACCORDION opens a band in place rather than replacing
/// the grid with it, so there is only ever the one kind now.
#[derive(Debug, Clone, Copy)]
struct BandRule {
    /// The band's place in `launcher::bands` — what its hit names.
    index: usize,
    /// The cursor is on the band: its branch goes bold.
    on: bool,
    /// The keys are on the band too — not up on the PROJECT TABS nor
    /// down in the pane: the rule takes the accent, the cursor mark
    /// and the Tab hint.
    lit: bool,
    /// Cards its row had no room for — always 0 once the band is open.
    more: usize,
    /// The band is open as the ACCORDION: its cards are wrapped into
    /// rows under this rule rather than the collapsed STRIP's one row,
    /// and the hint at the right end says Tab closes it.
    expanded: bool,
    /// The grid is the compact LIST: a collapsed band that already lists
    /// every entry has nothing for Tab to open, and its rule says nothing
    /// about it.
    list: bool,
}

/// A BAND's rule: the checkout — `⌂ main` or `↳ feat` in the SCOPE
/// COLOR, its uncommitted changes behind it in the heads-up color, its
/// pull request in the PR rows' own colors — a rule to the right end,
/// and there how many cards are under it (`2 sessions · 1 terminal`), or
/// off its edges (`▸ 2 more`). On the band the cursor is on the
/// branch is bold — this is what says which checkout the pane reads,
/// since no card under it wears the cursor — and for as long as the keys
/// are on it the rule wears the accent, opens on the CURSOR MARK `❯`
/// instead of `──`, and says at its right end what Tab does there
/// (`Tab: see all 8`, `Tab: expand`, or `Tab: collapse` on the one band
/// that is open); gray, unmarked and silent like the rest once the keys
/// are up on the PROJECT TABS or down in the pane.
/// Returns the rule's hits: the pull request ahead of the rule itself,
/// so a click on `#42` opens it and one anywhere else lands on the band.
fn draw_band_rule(
    buf: &mut Buffer,
    app: &App,
    r: Rect,
    band: &crate::launcher::Band,
    rule: BandRule,
) -> Vec<(Rect, HitTarget)> {
    let th = app.chrome.theme;
    let width = usize::from(r.width);
    let BandRule {
        on,
        lit,
        more,
        expanded,
        ..
    } = rule;
    // A session card dragged over this band: the rule lights and says
    // the release moves the card here.
    let drop = app.launcher.card_drag.as_ref().is_some_and(|d| {
        d.active && d.over_tab.is_none() && d.over.as_ref() == Some(&band.worktree)
    });
    let edge = if lit || drop { th.accent } else { th.edge };
    let dash = |n: usize| Span::styled("─".repeat(n), Style::default().fg(edge));

    // The right end first, since the left gives way to it.
    let right = {
        let mut words = format!("{} session{}", band.sessions(), plural(band.sessions()));
        if band.terminals() > 0 {
            words.push_str(&format!(
                " · {} terminal{}",
                band.terminals(),
                plural(band.terminals())
            ));
        }
        let mut spans = Vec::new();
        // WORKTREE PORTS its sessions are listening on, each with a live
        // dot: `:3010 ●` — the dev server is up, and `w` opens it.
        if let Some(info) = app.tree.ports.get(&band.worktree) {
            for port in &info.live {
                spans.push(Span::styled(
                    format!(":{port}"),
                    Style::default().fg(th.text),
                ));
                spans.push(Span::styled(" ● ", Style::default().fg(th.ok)));
                spans.push(Span::raw(" "));
            }
        }
        spans.push(Span::styled(words, Style::default().fg(th.dim)));
        if more > 0 {
            spans.push(Span::styled(
                format!("  ▸ {more} more"),
                Style::default().fg(th.muted),
            ));
        }
        // The VERB, on the band the keys are on: a titled rule reads as
        // a divider, and nothing about a divider says a key acts on it,
        // so the rule under the cursor spells what Tab does at its right
        // end. It names what the row could not show (`Tab: see all 8`)
        // when cards hang past the edge, plain `Tab: expand` when they
        // all fit collapsed, and `Tab: collapse` on the one band already
        // open. A rule too narrow to keep the branch legible beside the
        // words drops them: the `❯` at the left still says which band is
        // selected. Tab is the panels' "next panel" key, which the grid
        // takes for itself (`event_loop::launcher::handle_action`). With
        // every band open at once (**Expand all worktrees**) Tab has
        // nothing to open or fold, and no band says it does.
        let nothing_to_open =
            app.launcher.launcher_all_open || (rule.list && !expanded && more == 0);
        if lit && !nothing_to_open {
            let key = super::key_hint(app, crate::keymap::Action::FocusNext);
            let does = if expanded {
                ": collapse".to_string()
            } else if more > 0 {
                format!(": see all {}", band.cards.len())
            } else {
                ": expand".to_string()
            };
            let taken: usize = spans.iter().map(|s| s.width()).sum();
            let with_hint = 3 + taken + 2 + key.chars().count() + does.chars().count() + 4;
            if width >= with_hint + ENTER_HINT_ROOM {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(key, Style::default().fg(th.accent)));
                spans.push(Span::styled(does, Style::default().fg(th.dim)));
            }
        }
        spans
    };
    let right = if drop {
        vec![Span::styled(
            "release to move here",
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
        )]
    } else {
        right
    };
    let right_w: usize = right.iter().map(|s| s.width()).sum();

    // The checkout, as the cards used to paint it: the glyph in its
    // scope color never yields, the branch truncates around it.
    let (glyph, scope) = if band.is_main {
        ("⌂ ", th.root)
    } else {
        ("↳ ", th.worktree)
    };
    let mut branch_style = Style::default().fg(scope);
    if on {
        branch_style = branch_style.add_modifier(Modifier::BOLD);
    }
    // `── ` before, ` ` after the words, ` ` before the right end and
    // ` + ─` (the NEW BUTTON) after it: what the words have to fit between.
    let room = width.saturating_sub(3 + right_w + 6);
    let changes = app
        .worktree_changes(&band.worktree)
        .filter(|n| *n > 0)
        .map(|n| {
            change_labels(
                n,
                app.worktree_lines(&band.worktree),
                band.branch.chars().count(),
                room.saturating_sub(glyph.chars().count()),
            )
        });
    let taken = changes.as_ref().map_or(0, |(files, lines)| {
        files.chars().count()
            + lines.as_ref().map_or(0, |(added, removed)| {
                added.chars().count() + removed.chars().count()
            })
    });
    let branch = truncate(
        &band.branch,
        room.saturating_sub(glyph.chars().count() + taken),
    );
    // The rule's left end: `──` like every other band's, or the CURSOR
    // MARK `❯` on the one the keys are on — the mark a prompt puts before
    // the line that takes the keys, so a rule that looks like a divider
    // reads as a row something can be done to. Two cells either way, so
    // the checkouts stay in one column down the grid.
    let lead = if lit {
        Span::styled(
            "❯ ",
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
        )
    } else {
        dash(2)
    };
    let mut left = vec![
        lead,
        Span::raw(" "),
        Span::styled(glyph, Style::default().fg(scope)),
        Span::styled(branch, branch_style),
    ];
    if let Some((files, lines)) = changes {
        left.push(Span::styled(files, Style::default().fg(th.warn)));
        if let Some((added, removed)) = lines {
            left.push(Span::styled(added, Style::default().fg(th.ok)));
            left.push(Span::styled(removed, Style::default().fg(th.err)));
        }
    }
    let mut hits = Vec::new();
    let used: usize = left.iter().map(|s| s.width()).sum();
    // Its pull request, after the checkout, in the PR rows' own colors —
    // as much of the title as the rule has room for, and none of it on a
    // rule too short to say the number.
    if let Some(pr) = &band.pr {
        let spare = room.saturating_sub(used + 2 - 3);
        if spare >= 6 {
            let look = crate::pr_row::look(pr.standing, pr.trouble, th);
            let label = crate::pull_request::numbered_label(pr.number, &pr.title);
            let mut spans = crate::pr_row::spans(
                look,
                &label,
                spare,
                Some((format!(" {}", pr.badge()), look.badge)),
            );
            let hovered =
                app.launcher.hover_crumb == Some(HitTarget::LauncherBandPr(band.worktree.clone()));
            if hovered {
                if let Some(label) = spans.get_mut(1) {
                    label.style = label.style.add_modifier(Modifier::UNDERLINED);
                }
            }
            let pr_w: usize = spans.iter().map(|s| s.width()).sum();
            left.push(Span::raw("  "));
            let x = r.x + u16::try_from(used + 2).unwrap_or(u16::MAX);
            hits.push((
                Rect {
                    x,
                    width: u16::try_from(pr_w)
                        .unwrap_or(u16::MAX)
                        .min(r.width.saturating_sub(x - r.x)),
                    height: 1,
                    ..r
                },
                HitTarget::LauncherBandPr(band.worktree.clone()),
            ));
            left.extend(spans);
        }
    }
    let used: usize = left.iter().map(|s| s.width()).sum();
    let new_w = BAND_NEW.chars().count();
    let fill = width.saturating_sub(used + 1 + 1 + right_w + new_w + 1);
    let mut spans = left;
    spans.push(Span::raw(" "));
    spans.push(dash(fill));
    spans.push(Span::raw(" "));
    spans.extend(right);
    // The NEW BUTTON at the rule's right end: a session in this checkout,
    // the box `p` opens here — the grid had no pointer's way to start
    // one where you are looking. Only on a rule that fits it whole.
    let new_hit = HitTarget::LauncherBandNew(rule.index);
    let total = used + 1 + fill + 1 + right_w + new_w + 1;
    let shows_new = !drop && total <= width;
    if shows_new {
        let fg = if app.launcher.hover_crumb.as_ref() == Some(&new_hit) {
            th.accent
        } else {
            th.muted
        };
        spans.push(Span::styled(BAND_NEW, Style::default().fg(fg)));
        hits.push((
            Rect {
                x: r.x + (total - new_w - 1) as u16,
                width: new_w as u16,
                height: 1,
                ..r
            },
            new_hit,
        ));
    } else {
        spans.push(Span::raw(" "));
        spans.push(dash(new_w.saturating_sub(1)));
    }
    spans.push(dash(1));
    Paragraph::new(Line::from(spans)).render(r, buf);
    hits.push((r, HitTarget::LauncherBand(rule.index)));
    hits
}

/// A BAND rule's NEW BUTTON ([`draw_band_rule`]).
const BAND_NEW: &str = " + ";

/// A TERMINAL's card: what a session's card is to a session, and the
/// same size — its name behind the glyph the SESSIONS PANEL gave the row
/// (`▶` for a RUN TERMINAL, `❯` for a plain shell, in `th.ok` while its
/// PTY is alive, with `exited` at the row's right once the shell is
/// gone), what runs in it, then the last lines it printed where a
/// session's card has its prompt ([`terminal_tail_lines`]) — so a glance
/// down the grid says what each shell is up to, and an exited one what it
/// was doing when it went. Those lines are a small terminal
/// ([`draw_tail_row`]): the shell's own colours, clipped at the card's
/// edge, and a live shell's cursor. Framed and filled as a session's card
/// is when the cursor is on it ([`selected_card_block`]).
fn draw_chip(
    buf: &mut Buffer,
    app: &App,
    area: Rect,
    t: &nebula_core::TerminalTab,
    selected: bool,
    focused: bool,
    th: Theme,
) {
    let block = if selected {
        selected_card_block(app, focused, None, th)
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(th.edge))
    };
    let dim = if selected { th.muted } else { th.dim };
    let inner = block.inner(area);
    block.render(area, buf);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    let width = usize::from(inner.width);
    if width == 0 {
        return;
    }
    // HIDE CARD MARKS drops the glyph, and the name starts where it was.
    let glyph = if app.launcher.hide_card_marks {
        ""
    } else if t.run_command.is_some() {
        "▶ "
    } else {
        "❯ "
    };
    let glyph_w = glyph.chars().count();
    // `exited` sits where a session card keeps its ago badge, and the
    // name gives it the room, as a session's does.
    let badge = if t.alive { "" } else { EXITED_BADGE };
    let (badge, name_max) = fit_ago(badge.to_string(), width);
    let name = truncate(&t.name, name_max.saturating_sub(glyph_w));
    let mut first = vec![
        Span::styled(
            glyph,
            Style::default().fg(if t.alive { th.ok } else { dim }),
        ),
        Span::styled(
            name.clone(),
            Style::default().fg(th.text).add_modifier(Modifier::BOLD),
        ),
    ];
    if !badge.is_empty() {
        let badge = badge.trim_start().to_string();
        let pad = width.saturating_sub(name.chars().count() + glyph_w + badge.chars().count());
        first.push(Span::raw(" ".repeat(pad)));
        first.push(Span::styled(badge, Style::default().fg(th.err)));
    }
    let runs = t.run_command.as_deref().unwrap_or("shell");
    let second = vec![Span::styled(
        truncate(runs, width),
        Style::default().fg(dim),
    )];
    for (i, spans) in [first, second].into_iter().enumerate() {
        let Some(r) = row_rect(inner, i) else { break };
        Paragraph::new(Line::from(spans)).render(r, buf);
    }
    let head = usize::from(crate::launcher::CARD_HEAD_H);
    for (i, row) in terminal_tail_lines(app, t)
        .iter()
        .take(crate::launcher::PROMPT_LINES)
        .enumerate()
    {
        let Some(r) = row_rect(inner, head + i) else {
            break;
        };
        draw_tail_row(buf, r, row, t.alive, th);
    }
}

/// One row of a terminal card's tail, painted as the pane would paint
/// it: each run in the look the shell printed it in over the card's own
/// text colour and background — which is what a terminal's default is
/// on a card — cut at the card's right edge the way a narrower terminal
/// cuts, no `…`, and the shell's cursor a reversed cell while it lives:
/// over a character it reverses that character's own colours, past the
/// text it is a block in the card's text colour.
fn draw_tail_row(
    buf: &mut Buffer,
    r: Rect,
    row: &crate::terminal_tail::TailRow,
    alive: bool,
    th: Theme,
) {
    let base = Style::default().fg(th.text);
    let (mut x, right) = (r.x, r.right());
    for (text, look) in &row.runs {
        if x >= right {
            break;
        }
        x = buf
            .set_stringn(x, r.y, text, usize::from(right - x), base.patch(*look))
            .0;
    }
    let Some(col) = row.cursor.filter(|&c| alive && c < r.width) else {
        return;
    };
    let at = r.x + col;
    let look = if at < x { Style::default() } else { base };
    if let Some(cell) = buf.cell_mut((at, r.y)) {
        cell.set_style(look.add_modifier(Modifier::REVERSED));
    }
}

/// What a dead terminal's card says at its name row's right.
const EXITED_BADGE: &str = " exited";

/// The lines under a terminal card's head: the pane's own screen while the
/// pane is on this terminal — live, on the frame it changes — and
/// otherwise the tail the daemon last answered with
/// ([`App::terminal_tails`]), which a card keeps once its shell is gone.
fn terminal_tail_lines(
    app: &App,
    t: &nebula_core::TerminalTab,
) -> Vec<crate::terminal_tail::TailRow> {
    let sref = nebula_core::SessionRef::Terminal(t.id.clone());
    if let Some(term) = app
        .pane
        .term
        .as_ref()
        .filter(|term| term.sref == sref && term.painted)
    {
        return crate::terminal_tail::screen_tail(
            term.parser.screen(),
            crate::launcher::PROMPT_LINES,
        );
    }
    app.pane
        .terminal_tails
        .get(&t.id)
        .map(|tail| tail.lines.clone())
        .unwrap_or_default()
}

/// A card's frame color when its status wants one: red for a turn waiting
/// on you, blue for one finished and unread, yellow for one still running
/// — the three the PROJECT TABS count, each in the color its STATUS DOT
/// wears, and no other. A read finish, a fresh or terminated session and a
/// `quiet` card (cold, pending or archived: nothing on it is live) keep
/// the plain edge. The cursor's card outranks all three
/// ([`selected_card_block`]).
fn card_edge(a: &nebula_core::Agent, quiet: bool, th: Theme) -> Option<Color> {
    use nebula_core::AgentStatus;
    if quiet {
        return None;
    }
    match a.status {
        AgentStatus::NeedsFeedback => Some(th.err),
        AgentStatus::Finished if a.unseen => Some(th.done),
        AgentStatus::Running => Some(th.warn),
        _ => None,
    }
}

/// The frame of the card under the cursor, session or terminal: a heavy
/// accent border — a weight no status frame ([`card_edge`]) ever takes,
/// so a blue unread finish or a red question beside it can't be read as
/// the selection, color or no color — over the raised fill every selected
/// row in nebula wears, frame and all: `sel_bg` while the grid has the
/// keys, `sel_bg_dim` while the pane or the PROJECT TABS do, so the card
/// stays picked out as the one the pane reads.
///
/// HIGHLIGHT CURRENT CARD trades the gray fill for a faint wash of the
/// card's own light ([`card_tint`]), kept while the pane has the keys.
/// `status` is the color its frame would wear were it not selected
/// ([`card_edge`]); none for a quiet card or a terminal's.
fn selected_card_block(
    app: &App,
    focused: bool,
    status: Option<Color>,
    th: Theme,
) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(th.accent));
    let fill = if app.launcher.highlight_current_card {
        card_tint(app, status, th)
    } else if focused {
        th.sel_bg
    } else {
        th.sel_bg_dim
    };
    block.style(Style::default().bg(fill))
}

/// HIGHLIGHT CURRENT CARD's fill: the card's status color taken nearly to
/// black, so the card is only just washed in it. A card with something
/// going on — running, asking, finished and unread — breathes, the wash
/// rising and falling on the sweep's clock ([`tint_level`]); a quiet
/// card or a terminal's holds a still wash of the accent, as every card
/// does with the ANIMATIONS off. Fainter still while the PROJECT TABS
/// hold the keys.
fn card_tint(app: &App, status: Option<Color>, th: Theme) -> Color {
    let (color, level) = match status {
        Some(c) if app.chrome.animations => (c, tint_level(app.sweep_phase())),
        Some(c) => (c, TINT_PEAK),
        None => (th.accent, TINT_STILL),
    };
    let away = if app.launcher.launcher_tab_cursor.is_none() {
        1.0
    } else {
        0.6
    };
    th.toward_bg(color, level * away)
}

/// How much of its status color a breathing card's fill keeps at its
/// brightest, and how much a still one keeps of the accent.
const TINT_PEAK: f32 = 0.20;
const TINT_STILL: f32 = 0.13;
/// Its dimmest: all but the grid's own black.
const TINT_FLOOR: f32 = 0.07;
/// Sweep frames ([`crate::app::SWEEP_FRAME`]) in one breath, ~1.6 s.
const TINT_BREATH: usize = 16;

/// Where a breathing fill is at sweep `phase`: a cosine from
/// [`TINT_FLOOR`] up to [`TINT_PEAK`] and back.
fn tint_level(phase: usize) -> f32 {
    let t = (phase % TINT_BREATH) as f32 / TINT_BREATH as f32;
    let wave = (1.0 - (t * std::f32::consts::TAU).cos()) / 2.0;
    TINT_FLOOR + (TINT_PEAK - TINT_FLOOR) * wave
}

/// `c` at `level` of its brightness, the rest black — what a wash of `c`
/// is on a theme with no canvas of its own ([`Theme::toward_bg`] is the
/// general case, a TRUECOLOR PRESET's wash running toward its own
/// canvas instead).
#[cfg(test)]
fn dim_toward_black(c: Color, level: f32) -> Color {
    crate::theme::blend(c, None, level)
}

/// What an ARCHIVED card wears where a live one wears its STATUS DOT: the
/// round dot squared off. Two columns wide like the dot it stands in for,
/// so the name behind it starts in the same column on both grids.
const ARCHIVED_MARK: &str = "▪ ";

/// One session's card: its name and how long since it last moved, what
/// it runs on, and the last thing it was asked to do. Where it runs —
/// the checkout, its changes, its pull request — is on its BAND's rule,
/// said once for every card in the checkout. The cursor's card is raised
/// out of the grid wherever the keys are ([`selected_card_block`]); every
/// other card's frame answers to its status ([`card_edge`]).
#[allow(clippy::too_many_arguments)]
fn draw_card(
    buf: &mut Buffer,
    app: &App,
    area: Rect,
    row: &LauncherRow,
    selected: bool,
    focused: bool,
    th: Theme,
    cfg: &mut Option<crate::config::Config>,
) {
    let a = &row.agent;
    let pending = app.is_placeholder_agent(&a.id);
    let cold = !a.alive && a.cloud_session_id.is_none();
    let archived = a.archived;
    let SessionLook {
        dot,
        quiet,
        name_style,
        ramp,
        ago,
        ago_style,
    } = session_look(app, a, selected, th);
    let quiet_or = |live: Color| if archived { quiet } else { live };
    // On the raised fill the dim parts step up to muted and the prompt to
    // text, so nothing on the cursor's card sinks into its background.
    let (dim, prompt) = if selected {
        (th.muted, th.text)
    } else {
        (th.dim, th.muted)
    };
    let edge = card_edge(a, archived || pending || cold, th);
    let block = if selected {
        selected_card_block(app, focused, edge, th)
    } else {
        Block::default()
            .borders(Borders::ALL)
            // Square corners on an archived card, round on a live one: the
            // one difference between the two grids that survives a
            // terminal with no color at all.
            .border_type(if archived {
                BorderType::Plain
            } else {
                BorderType::Rounded
            })
            .border_style(Style::default().fg(edge.unwrap_or(th.edge)))
    };
    let inner = block.inner(area);
    block.render(area, buf);
    // One cell of air inside the border, so the text never touches it.
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    let width = inner.width as usize;
    if width == 0 {
        return;
    }

    let (ago, name_max) = fit_ago(ago, width);
    let name = truncate(&a.name, name_max.saturating_sub(2));
    let mut first = vec![dot];
    let used = name.chars().count() + 2;
    first.extend(status_name_spans(name, name_style, ramp, app.sweep_phase()));
    if !ago.is_empty() {
        let ago = ago.trim_start().to_string();
        let pad = width.saturating_sub(used + ago.chars().count());
        first.push(Span::raw(" ".repeat(pad)));
        first.push(Span::styled(ago, ago_style));
    }

    // What it runs on: the harness, the model and the reasoning effort
    // the session was launched with, dim. Where it runs — the checkout,
    // its changes, its pull request — is the BAND's rule over the card,
    // said once for every card in the checkout rather than on each. The
    // project is not on the card either: the grid is one project's,
    // named in the header.
    let harness = runs_on_line(a, cfg);
    let mut second = if harness.is_empty() {
        Vec::new()
    } else {
        vec![Span::styled(
            truncate(&harness, width),
            Style::default().fg(quiet_or(dim)),
        )]
    };
    // The issue it was started from, at the row's right end — a link, and
    // the only thing on the card under the pointer that is not the card
    // (`HitTarget::LauncherCardIssue`, placed by `card_issue_rect`).
    if let Some(label) = card_issue_label(app, a) {
        let label_w = label.chars().count();
        if card_issue_rect(area, label_w).is_some() {
            let room = width - label_w - 1;
            let harness = truncate(&harness, room);
            let pad = width - label_w - harness.chars().count();
            let mut style = Style::default().fg(quiet_or(th.accent));
            if app.launcher.hover_crumb == Some(HitTarget::LauncherCardIssue(a.id.clone())) {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            second = vec![
                Span::styled(harness, Style::default().fg(quiet_or(dim))),
                Span::raw(" ".repeat(pad)),
                Span::styled(label, style),
            ];
        }
    }

    // The session's CONTEXT: `ctx 42%` (and `⟳2` after two compactions)
    // at the row's right end, in its tone — a card mark, so
    // `hide_card_marks` leaves it off, and the issue number keeps the spot.
    if !app.launcher.hide_card_marks && card_issue_label(app, a).is_none() {
        if let Some((label, tone)) = app
            .launcher
            .session_contexts
            .get(&a.id)
            .and_then(|c| Some((crate::accounts_overlay::context_label(c)?, c.percent?)))
        {
            let label_w = label.chars().count();
            if width > label_w + 4 {
                let room = width - label_w - 1;
                let harness = truncate(&harness, room);
                let pad = width - label_w - harness.chars().count();
                let fg = match nebula_core::usage::tone(tone) {
                    nebula_core::usage::Tone::Ok => quiet_or(dim),
                    nebula_core::usage::Tone::Watch => quiet_or(th.accent),
                    nebula_core::usage::Tone::Warn => quiet_or(th.warn),
                    nebula_core::usage::Tone::Hot => quiet_or(th.err),
                };
                second = vec![
                    Span::styled(harness, Style::default().fg(quiet_or(dim))),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(label, Style::default().fg(fg)),
                ];
            }
        }
    }

    // Why its last turn stopped short — `resumes 3:45pm`, `logged out` —
    // takes that same spot ahead of the issue number and the context: it
    // is the fact on the card that says whether to wait or to come back.
    // A card mark too, and a card too narrow for it keeps what it had.
    if let Some((label, fg)) = card_stop(app, a, width, th) {
        let label_w = label.chars().count();
        let room = width - label_w - 1;
        let harness = truncate(&harness, room);
        let pad = width - label_w - harness.chars().count();
        second = vec![
            Span::styled(harness, Style::default().fg(quiet_or(dim))),
            Span::raw(" ".repeat(pad)),
            Span::styled(label, Style::default().fg(quiet_or(fg))),
        ];
    }

    // The ORCHESTRATOR BADGE: the machine's one orchestrator says so ahead
    // of what it runs on (F10 opens it).
    if a.role.is_orchestrator() {
        second.insert(
            0,
            Span::styled(
                ORCHESTRATOR_BADGE,
                Style::default()
                    .fg(quiet_or(th.accent))
                    .add_modifier(Modifier::BOLD),
            ),
        );
    }

    // The last thing it was asked to do, on the prompt's own `›` (which
    // `hide_card_marks` leaves off), over the card's last rows rather than
    // clipped at the first.
    // Its SUBAGENT LINES take the prompt's last rows: what the session
    // has working for it right now is worth more than the prompt's tail.
    let subagents = super::subagents_view::card_lines(app, &a.id, width, archived, th);
    let prompt_rows = crate::launcher::PROMPT_LINES - subagents.len();
    let mut lines = vec![first, second];
    lines.resize(crate::launcher::CARD_HEAD_H as usize, Vec::new());
    let mut prompt = prompt_lines(
        crate::launcher::last_prompt(a).unwrap_or_default(),
        width,
        (!app.launcher.hide_card_marks).then(|| quiet_or(dim)),
        quiet_or(prompt),
    );
    prompt.resize(prompt_rows, Vec::new());
    lines.extend(prompt);
    lines.extend(subagents);

    for (i, spans) in lines.into_iter().enumerate() {
        if spans.is_empty() {
            continue;
        }
        let Some(r) = row_rect(inner, i) else { break };
        Paragraph::new(Line::from(spans)).render(r, buf);
    }
}

/// The CARD BUTTONS on the bottom edge of the card under the cursor, as
/// (label, target), most wanted first: a narrow card keeps the head of
/// the list. The `⋯` is first of all — it holds every verb the rest do —
/// though it is drawn last, at the right end.
fn card_button_list(
    card: &crate::launcher::Card,
    on: &ButtonsOn,
) -> Vec<(&'static str, HitTarget)> {
    use crate::keymap::Action;
    let act = |label, action| (label, on.button(action));
    let mut list = vec![(CARD_MENU, on.menu())];
    match card {
        crate::launcher::Card::Session(row) if row.agent.archived => {
            list.push(act(" unarchive ", Action::Unarchive));
            list.push(act(" delete ", Action::Delete));
        }
        // A board card is narrow — a fifth of the width — and is there to
        // be read and acted on in place: its diff comes first.
        crate::launcher::Card::Session(_) if matches!(on, ButtonsOn::Board(_)) => {
            list.push(act(" diff ", Action::GitDiff));
            list.push(act(" reply ", Action::FollowUp));
            list.push(act(" archive ", Action::Archive));
            list.push(act(" delete ", Action::Delete));
        }
        crate::launcher::Card::Session(_) => {
            list.push(act(" reply ", Action::FollowUp));
            list.push(act(" delete ", Action::Delete));
            list.push(act(" archive ", Action::Archive));
            list.push(act(" diff ", Action::GitDiff));
        }
        crate::launcher::Card::Terminal(_) => {
            list.push(act(" close ", Action::Delete));
            list.push(act(" diff ", Action::GitDiff));
        }
    }
    list
}

/// Which card the CARD BUTTONS are on: a band's, by its place on the grid,
/// or a board card, by its session — so a click reaches the very card it
/// was drawn on in either view.
pub(super) enum ButtonsOn {
    Grid(crate::launcher::CardRef),
    Board(nebula_core::AgentId),
}

impl ButtonsOn {
    fn button(&self, action: crate::keymap::Action) -> HitTarget {
        match self {
            ButtonsOn::Grid(at) => HitTarget::CardButton(*at, action),
            ButtonsOn::Board(id) => HitTarget::KanbanButton(id.clone(), action),
        }
    }

    fn menu(&self) -> HitTarget {
        match self {
            ButtonsOn::Grid(at) => HitTarget::CardMenu(*at),
            ButtonsOn::Board(id) => HitTarget::KanbanMenu(id.clone()),
        }
    }
}

/// The order the CARD BUTTONS read in, left to right; the `⋯` last.
fn card_button_rank(hit: &HitTarget) -> usize {
    use crate::keymap::Action;
    let action = match hit {
        HitTarget::CardButton(_, action) | HitTarget::KanbanButton(_, action) => *action,
        _ => return 4,
    };
    match action {
        Action::FollowUp | Action::Unarchive => 0,
        Action::GitDiff => 1,
        Action::Archive => 2,
        Action::Delete => 3,
        _ => 4,
    }
}

/// The CARD BUTTONS of the card `on`, drawn at `placed`, laid out on
/// its bottom border: right-aligned inside the corners, each whole or not
/// at all ([`card_button_list`] says which go first), and none on a card
/// whose bottom row the window cut off.
pub(super) fn card_buttons(
    card: &crate::launcher::Card,
    on: &ButtonsOn,
    placed: crate::launcher::Placed,
    reserve: usize,
) -> Vec<(Rect, HitTarget, &'static str)> {
    let r = placed.rect;
    if placed.cut_bottom > 0 || r.height < 3 || r.width < 8 {
        return Vec::new();
    }
    // A corner and a cell of rule at either end stay the card's, and the
    // left end's SUBAGENT CHIP keeps its `reserve`: buttons give way first.
    let room = (usize::from(r.width) - 4).saturating_sub(reserve);
    let mut kept: Vec<(&'static str, HitTarget)> = Vec::new();
    let mut used = 0;
    for (label, hit) in card_button_list(card, on) {
        let w = label.chars().count();
        if used + w > room {
            break;
        }
        used += w;
        kept.push((label, hit));
    }
    kept.sort_by_key(|(_, hit)| card_button_rank(hit));
    let mut x = r.x + r.width - 2 - used as u16;
    let y = r.y + r.height - 1;
    kept.into_iter()
        .map(|(label, hit)| {
            let width = label.chars().count() as u16;
            let rect = Rect {
                x,
                y,
                width,
                height: 1,
            };
            x += width;
            (rect, hit, label)
        })
        .collect()
}

/// Draw [`card_buttons`] over the card's bottom border: muted, the one
/// under the pointer in the accent and underlined, as every button on the
/// grid is.
pub(super) fn draw_card_buttons(
    buf: &mut Buffer,
    app: &App,
    buttons: &[(Rect, HitTarget, &'static str)],
) {
    let th = app.chrome.theme;
    for (rect, hit, label) in buttons {
        let style = if app.launcher.hover_crumb.as_ref() == Some(hit) {
            Style::default()
                .fg(th.accent)
                .add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        buf.set_stringn(rect.x, rect.y, label, usize::from(rect.width), style);
    }
}

/// The card under the cursor's CARD BUTTONS, drawn and registered ahead of
/// the card's own hit so they win `hit_at`'s first-match scan. Nothing on
/// any other card, or with the aim let go: the buttons are the cursor's.
fn selected_card_buttons(
    f: &mut Frame,
    app: &mut App,
    card: &crate::launcher::Card,
    at: crate::launcher::CardRef,
    placed: crate::launcher::Placed,
    selected: bool,
) {
    if !selected || app.launcher.launcher_unaimed {
        return;
    }
    let reserve = super::subagents_view::card_chip_width(app, card);
    let buttons = card_buttons(card, &ButtonsOn::Grid(at), placed, reserve);
    draw_card_buttons(f.buffer_mut(), app, &buttons);
    app.chrome
        .hits
        .extend(buttons.into_iter().map(|(rect, hit, _)| (rect, hit)));
}

/// The `⋯` CARD BUTTON: the card's menu.
const CARD_MENU: &str = " ⋯ ";

/// What the orchestrator's card says ahead of its harness.
pub(crate) const ORCHESTRATOR_BADGE: &str = "◆ orchestrator ";

/// The `#15` an ISSUE SESSION's card shows for the GitHub issue it was
/// started from, while the `card_issue_number` setting is on. None on
/// every other card, and on every card with the setting off.
fn card_issue_label(app: &App, a: &nebula_core::Agent) -> Option<String> {
    if !app.launcher.card_issue_number {
        return None;
    }
    a.issue_number().map(|n| format!("#{n}"))
}

/// The color a stop note's words go in: a wait the session carries out
/// by itself is the running color, never the needs-you one; a note that
/// needs the user is the needs-you color; a turn that ended on an error
/// is the stopped color.
fn stop_color(tone: nebula_core::stop::StopTone, th: Theme) -> Color {
    use nebula_core::stop::StopTone;
    match tone {
        StopTone::Waiting => th.warn,
        StopTone::NeedsYou => th.err,
        StopTone::Error => th.special,
    }
}

/// What session `a`'s last stop note says, and the color to say it in:
/// `resumes 3:45pm`, `logged out`. None while it has no note.
fn stop_look(app: &App, a: &nebula_core::Agent, th: Theme) -> Option<(String, Color)> {
    let note = app.launcher.session_stops.get(&a.id)?;
    let now = nebula_core::clock::now_secs() as i64;
    Some((note.label(now), stop_color(note.tone(), th)))
}

/// The stop note a card of inner `width` columns says at the right end
/// of its second row ([`stop_look`]). None with the card marks hidden,
/// and on a card too narrow to hold it with the harness text and a cell
/// to spare.
fn card_stop(
    app: &App,
    a: &nebula_core::Agent,
    width: usize,
    th: Theme,
) -> Option<(String, Color)> {
    if app.launcher.hide_card_marks {
        return None;
    }
    let (label, fg) = stop_look(app, a, th)?;
    (width > label.chars().count() + 4).then_some((label, fg))
}

/// Where a card drawn whole at `area` puts its issue number `label_w`
/// cells wide: the right end of the row saying what it runs on, inside
/// the border and its cell of air (`draw_card`). None on a card too
/// small to hold it with a cell to spare.
fn card_issue_rect(area: Rect, label_w: usize) -> Option<Rect> {
    let inner = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let label_w = u16::try_from(label_w).ok()?;
    if inner.height < 2 || inner.width <= label_w {
        return None;
    }
    Some(Rect {
        x: inner.x + inner.width - label_w,
        y: inner.y + 1,
        width: label_w,
        height: 1,
    })
}

/// The click target of the issue number on the session card `card`,
/// placed at `placed` (cut or whole): the chip's cell when that row of
/// the card is on screen, for [`draw_bands`] to register ahead of the
/// card so the chip wins.
fn card_issue_hit(
    app: &App,
    card: &crate::launcher::Card,
    placed: crate::launcher::Placed,
) -> Option<(Rect, HitTarget)> {
    let crate::launcher::Card::Session(row) = card else {
        return None;
    };
    let label = card_issue_label(app, &row.agent)?;
    // A stop note drawn over the chip leaves nothing there to click; the
    // card's inner width is its own less the border and its cell of air.
    let inner_w = usize::from(placed.rect.width).saturating_sub(4);
    if card_stop(app, &row.agent, inner_w, app.chrome.theme).is_some() {
        return None;
    }
    // The chip on the whole card laid out from row 0, then its row moved
    // to the screen — kept only while the window shows that row.
    let full = Rect {
        y: 0,
        height: placed.rect.height + placed.cut_top + placed.cut_bottom,
        ..placed.rect
    };
    let chip = card_issue_rect(full, label.chars().count())?;
    let row_on_screen = chip.y.checked_sub(placed.cut_top)?;
    (row_on_screen < placed.rect.height).then(|| {
        (
            Rect {
                y: placed.rect.y + row_on_screen,
                ..chip
            },
            HitTarget::LauncherCardIssue(row.agent.id.clone()),
        )
    })
}

/// How a session shows its state wherever the grid draws it — a card's
/// head, a LIST entry — so the two never disagree.
struct SessionLook {
    /// The STATUS DOT, or an archived card's square.
    dot: Span<'static>,
    /// What an archived session's every part is drawn in, and a live
    /// one's age: dim, a step up on the cursor's entry.
    quiet: Color,
    name_style: Style,
    /// The status sweep across the name, while it animates.
    ramp: Option<[Color; 3]>,
    /// How long since it moved (`done`, `starting`, …), with its lead space.
    ago: String,
    ago_style: Style,
}

/// `a`'s [`SessionLook`], on the cursor's entry when `selected`.
///
/// An ARCHIVED session is the same session put away, and it is drawn as
/// such: nothing on it is live, so nothing on it is colored. Every part of
/// it takes `quiet` — dim on its own, lifted to muted on the one the
/// cursor is on, so it reads a step brighter than its neighbours — its
/// name the plain `muted`, its STATUS DOT gives way to `ARCHIVED_MARK`,
/// and a card's frame squares off (`draw_card`). The colors say it at a
/// glance and the shapes say it again with the colors off, which is the
/// whole grid's answer to "which of the two lists am I looking at" without
/// a word repeated on every card - the header's `n archived sessions` says
/// that once.
fn session_look(app: &App, a: &nebula_core::Agent, selected: bool, th: Theme) -> SessionLook {
    let pending = app.is_placeholder_agent(&a.id);
    let cold = !a.alive && a.cloud_session_id.is_none();
    let archived = a.archived;
    let quiet = if selected { th.muted } else { th.dim };
    // The dot and the sweep read the session's status, cold and pending
    // alike.
    let dot = if archived {
        Span::styled(ARCHIVED_MARK, Style::default().fg(quiet))
    } else if pending {
        status_dot(None, false, th)
    } else if cold {
        Span {
            style: Style::default().fg(th.dim),
            ..status_dot(Some(a.status), false, th)
        }
    } else {
        status_dot(Some(a.status), a.unseen, th)
    };
    let ramp = if pending || cold || archived {
        None
    } else {
        sweep_ramp(
            Some(a.status),
            app.agent_fresh_done(a),
            th,
            app.chrome.animations,
        )
    };
    // An archived name is not the loud thing on the screen any more: it
    // gives up the bold with the rest of the card's weight and sits one
    // step above the quiet the rest of the card is in — muted over dim,
    // and text over muted on the card the cursor is on, the same one-step
    // lift `quiet` takes there.
    let name_style = if archived {
        Style::default().fg(if selected { th.text } else { th.muted })
    } else {
        Style::default().fg(th.text).add_modifier(Modifier::BOLD)
    };
    // How long ago it was filed, on an archived card, rather than when its
    // turn last moved: the status behind it stopped being news the moment
    // it was put away. A row archived before the stamp existed carries 0
    // and simply has no badge.
    let ago = if archived {
        ago_badge(a.archived_at)
    } else if pending {
        PENDING_SESSION_BADGE.to_string()
    } else if a.unseen {
        " done".to_string()
    } else {
        ago_badge(a.status_changed_at)
    };
    let ago_style = if a.unseen && !pending && !archived {
        Style::default().fg(th.done)
    } else {
        Style::default().fg(quiet)
    };
    SessionLook {
        dot,
        quiet,
        name_style,
        ramp,
        ago,
        ago_style,
    }
}

/// Which card wears the cursor: the one the cursor is on, or none at all
/// once the aim has been let go of (`event_loop::launcher::clear_aim` —
/// a click on the air between the cards, or the first Esc). The grid
/// still knows where the cursor is; it simply draws no card as selected,
/// which is what says the box `p` opens will ask where its session lands.
fn wearing(app: &App, cursor: Option<usize>) -> Option<usize> {
    (!app.launcher.launcher_unaimed).then_some(cursor).flatten()
}

/// What the ARCHIVED VIEW shows with no cards at all — one line where the
/// grid would be, rather than the live list's hero, which is about
/// starting a session and has nothing to say here.
fn draw_list_empty(f: &mut Frame, app: &mut App, area: Rect, what: &str) {
    app.chrome
        .hits
        .push((area, HitTarget::PanelBg(Focus::Sessions)));
    if app.modals.overlay.is_some() {
        return;
    }
    let th = app.chrome.theme;
    if let Some(r) = row_rect(area, 1) {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                what.to_string(),
                Style::default().fg(th.dim),
            )))
            .alignment(ratatui::layout::Alignment::Center),
            r,
        );
    }
}

/// The prompt block at the foot of a card: the `›` on its first row and
/// the sentence wrapped under it, indented to the same column, over at
/// most [`crate::launcher::PROMPT_LINES`] rows — so a card is a fixed
/// height whatever it was asked to do. A prompt longer than that is cut
/// on the last of them with an ellipsis; an empty one draws nothing.
/// `mark` is the `›`'s color, `None` (HIDE CARD MARKS) to leave it off
/// and start every row in its column.
fn prompt_lines(
    prompt: &str,
    width: usize,
    mark: Option<Color>,
    text: Color,
) -> Vec<Vec<Span<'static>>> {
    const MARK: &str = "› ";
    let indent = if mark.is_some() {
        MARK.chars().count()
    } else {
        0
    };
    let body = width.saturating_sub(indent);
    if prompt.is_empty() || body == 0 {
        return Vec::new();
    }
    let wrapped = crate::pr_preview::wrap(prompt, body);
    let keep = crate::launcher::PROMPT_LINES;
    let cut = wrapped.len() > keep;
    wrapped
        .into_iter()
        .take(keep)
        .enumerate()
        .map(|(i, line)| {
            let last = i + 1 == keep;
            let line_text = if last && cut {
                truncate(&format!("{line}…"), body)
            } else {
                line
            };
            let lead = match mark {
                Some(mark) if i == 0 => Span::styled(MARK, Style::default().fg(mark)),
                _ => Span::raw(" ".repeat(indent)),
            };
            vec![lead, Span::styled(line_text, Style::default().fg(text))]
        })
        .collect()
}

/// The harness (and model) a session runs on, as the PANE's breadcrumb
/// names it: `claude opus`. A Claude Cloud row says `cloud` — the sandbox
/// is the harness that matters there. The card adds the reasoning effort
/// through [`runs_on_line`].
///
/// `cfg` is the frame's CONFIG.JSON slot, filled on the first card that
/// needs it: only a CUSTOM harness's label comes out of the file, so a
/// grid of built-ins never opens it at all.
/// What a card's branch row says about its checkout's uncommitted changes,
/// beside a branch `branch` characters long in `room`: the file count —
/// ` +3 files`, or ` +3` — and, with `lines` counted, the lines behind it,
/// ` +120` and ` -45`. The word yields first, then the lines, and only then
/// does the branch give up a letter.
fn change_labels(
    files: usize,
    lines: Option<crate::git_diff::LineChanges>,
    branch: usize,
    room: usize,
) -> (String, Option<(String, String)>) {
    let long = format!(" +{files} file{}", if files == 1 { "" } else { "s" });
    let short = format!(" +{files}");
    let width = |s: &str| s.chars().count();
    if let Some(l) = lines {
        let (added, removed) = (format!(" +{}", l.added), format!(" -{}", l.removed));
        let tail = width(&added) + width(&removed);
        for count in [&long, &short] {
            if branch + width(count) + tail <= room {
                return (count.clone(), Some((added, removed)));
            }
        }
    }
    let count = if branch + width(&long) <= room {
        long
    } else {
        short
    };
    (count, None)
}

fn harness_line(a: &nebula_core::Agent, cfg: &mut Option<crate::config::Config>) -> String {
    if a.cloud_session_id.is_some() {
        return "cloud".into();
    }
    let mut out = if a.kind == nebula_core::AgentKind::Custom {
        let cfg = cfg.get_or_insert_with(crate::config::Config::load);
        crate::agent_picker::session_harness_badge_in(a, cfg)
    } else {
        a.kind.as_str().to_string()
    };
    // The PROFILE a session runs on, whenever it has one: `claude@max2
    // opus`. The default account is named too, so the label never flips
    // when the default moves to another account — only a session on the
    // CLI's own login (no profile) reads as the bare harness.
    if let Some(profile) = a.profile.as_deref().filter(|p| !p.trim().is_empty()) {
        out.push('@');
        out.push_str(profile.trim());
    }
    if let Some(model) = a.model.as_deref().filter(|m| !m.is_empty()) {
        out.push(' ');
        out.push_str(model);
    }
    out
}

/// What a session's card says it runs on: [`harness_line`] with the
/// reasoning effort the session was launched with after the model —
/// `claude opus high`, `codex gpt-5.5 xhigh`. A session on the CLI's own
/// default effort names none, so the line is the harness line alone
/// rather than one with a hole in it.
fn runs_on_line(a: &nebula_core::Agent, cfg: &mut Option<crate::config::Config>) -> String {
    let mut out = harness_line(a, cfg);
    if let Some(effort) = a.effort.as_deref().filter(|e| !e.is_empty()) {
        out.push(' ');
        out.push_str(effort);
    }
    out
}

/// Smallest GRID the welcome turns a nebula in: under it the sky would be
/// a few smudges of dust, so the words stand alone, centered.
const SKY_MIN_W: u16 = 30;
const SKY_MIN_H: u16 = 10;

/// The grid with nothing in it: the splash's animated nebula across the
/// space the cards would take, and one welcome under its core — the name,
/// and the one key that starts a session, as a key cap a click presses
/// too ([`HitTarget::LauncherWelcomePrompt`], through the same
/// `event_loop::launcher::open_box` the key runs). The key is whatever
/// the keymap binds, so a rebound prompt is never advertised as `p`.
fn draw_empty(f: &mut Frame, app: &mut App, area: Rect) {
    // Under the box (or any modal) the welcome would only peek out around
    // its edges in fragments; the box is saying the same thing.
    if app.modals.overlay.is_some() {
        app.chrome
            .hits
            .push((area, HitTarget::PanelBg(Focus::Sessions)));
        return;
    }
    let th = app.chrome.theme;
    let t = crate::splash::scene_time(app, app.chrome.splash_epoch);
    let mut welcome = vec![Span::styled("Welcome to ", Style::default().fg(th.text))];
    welcome.extend(crate::splash::wordmark_word("kete", t));
    // The key line is a button, and marked as one under the pointer the
    // way the header's are.
    let mut words = Style::default().fg(th.muted);
    if app.launcher.hover_crumb == Some(HitTarget::LauncherWelcomePrompt) {
        words = words.add_modifier(Modifier::UNDERLINED);
    }
    let key = super::key_hint(app, crate::keymap::Action::QuickPrompt);
    let prompt = Line::from(vec![
        Span::styled("press ", words),
        Span::styled(
            format!(" {key} "),
            Style::default()
                .fg(th.on_accent)
                .bg(th.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to prompt", words),
    ]);
    let prompt_w = (prompt.width() as u16).min(area.width);
    let lines = vec![Line::from(welcome), Line::from(""), prompt];
    let w = (lines.iter().map(Line::width).max().unwrap_or(0) as u16).min(area.width);
    let h = (lines.len() as u16).min(area.height);
    // The words sit a little under the middle, leaving the galaxy the sky
    // above them to turn in; with no room for one they are just centered.
    let sky = area.width >= SKY_MIN_W && area.height >= SKY_MIN_H;
    let y = if sky {
        area.y + (area.height * 3 / 5).saturating_sub(h / 2)
    } else {
        area.y + area.height.saturating_sub(h) / 2
    };
    let text = Rect {
        x: area.x + (area.width - w) / 2,
        y,
        width: w,
        height: h,
    };
    if sky {
        // A wider berth than the splash's: here the arms curl down past
        // the words, where the splash's run out above them.
        let clear = Rect {
            x: text.x.saturating_sub(4),
            width: text.width + 8,
            ..text
        }
        .intersection(area);
        crate::splash::draw_sky(f.buffer_mut(), area, clear, t, th.accent);
    }
    // Even with no sky, the name's shine moves.
    app.chrome.welcome_on_screen = true;
    f.render_widget(Paragraph::new(lines).centered(), text);
    // Ahead of the background, so it wins `hit_at`'s first-match scan.
    let key_row = Rect {
        x: area.x + (area.width - prompt_w) / 2,
        y: text.y + 2,
        width: prompt_w,
        height: 1,
    }
    .intersection(area);
    app.chrome
        .hits
        .push((key_row, HitTarget::LauncherWelcomePrompt));
    app.chrome
        .hits
        .push((area, HitTarget::PanelBg(Focus::Sessions)));
}

/// One tab in the PANE's TAB STRIP, or in the header's PROJECT TABS: what
/// it draws, and what a click on it means. The gaps and the divider
/// between tabs are tabs with no hit of their own, so one walk both lays
/// the strip out and hit-tests it.
struct PaneTab {
    spans: Vec<Span<'static>>,
    hit: Option<HitTarget>,
}

impl PaneTab {
    fn plain(spans: Vec<Span<'static>>) -> Self {
        Self { spans, hit: None }
    }

    fn width(&self) -> usize {
        self.spans.iter().map(|s| s.content.chars().count()).sum()
    }
}

/// The PANE's CLOSE BUTTON, at the right end of its header: one click
/// folds the pane away, as `^~` does. A cell of air either side, so the
/// target is wider than the glyph.
const PANE_CLOSE: &str = " × ";
/// The PANE's SIDE BUTTON, just before the CLOSE BUTTON: a picture of the
/// layout one click moves it to — the pane down the right of the cards on
/// a pane along the bottom, the pane along the bottom on one down the
/// right. The click writes Settings → Appearance → **Session pane**, so
/// the move lasts. Aired like the close button.
const PANE_TO_RIGHT: &str = " ◨ ";
const PANE_TO_BOTTOM: &str = " ⬓ ";
/// The PANE's FULL-SCREEN BUTTON, before the SIDE BUTTON: one click gives
/// the session the whole screen, as `^F` does. A full-screen session's
/// header ends in the NORMAL-SIZE BUTTON, which brings it back down.
const PANE_FULL_SCREEN: &str = " ⤢ ";
const PANE_NORMAL_SIZE: &str = " ⤡ ";
/// The PANE's DIFF BUTTON: the checkout's diff, the modal `g` opens.
const PANE_DIFF: &str = " ± ";
/// The PANE's MENU BUTTON: the card's CONTEXT MENU, a right-click's.
const PANE_MENU: &str = " ⋯ ";

/// Is the PANE reading a card on the grid — the one the cursor is on —
/// rather than a session the grid no longer lists?
fn pane_reads_card(app: &App) -> bool {
    let bands = crate::launcher::bands(app);
    crate::launcher::cursor(app, &bands)
        .and_then(|at| crate::launcher::card_at(&bands, at))
        .is_some()
}

/// A header button of `glyph` at `rect`, muted until the pointer is on
/// it and lit in `lit` then, registered as `hit`.
fn header_button(
    f: &mut Frame,
    app: &mut App,
    rect: Rect,
    glyph: &'static str,
    hit: HitTarget,
    lit: ratatui::style::Color,
) {
    let fg = if app.launcher.hover_crumb.as_ref() == Some(&hit) {
        lit
    } else {
        app.chrome.theme.muted
    };
    f.render_widget(
        Paragraph::new(Span::styled(glyph, Style::default().fg(fg))),
        rect,
    );
    app.chrome.hits.push((rect, hit));
}

/// The PANE's own header in the LAUNCHER VIEW: what the pane is reading
/// — the card under the cursor, session or terminal, and the checkout
/// it runs in — then the state tag (`scroll 4`, `exited`) right-aligned
/// before the FULL-SCREEN, SIDE and CLOSE BUTTONS. A rule under it, and the rect the
/// PTY draws in returned, exactly as `ui::terminal_frame` does for the
/// panels' pane. Opening a terminal is `t`'s alone: the grid offers no
/// `+` for it.
pub(super) fn pane_frame(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    right: Option<Span<'static>>,
    focused: bool,
) -> Rect {
    let th = app.chrome.theme;
    if let Some(r) = row_rect(area, 1) {
        let r = pad_x(r);
        // The CLOSE BUTTON holds the right end of the row, the SIDE
        // BUTTON stands before it — unless there is nowhere to move the
        // pane (`App::launcher_pane_move_to`) — and the state tag
        // (`scroll 4`, exited) is right-aligned before those: the title
        // gets what the three leave.
        let side_button = app.launcher_pane_move_to().map(|to| match to {
            crate::launcher::PaneSide::Right => PANE_TO_RIGHT,
            crate::launcher::PaneSide::Bottom => PANE_TO_BOTTOM,
        });
        let zoom_w = PANE_FULL_SCREEN.chars().count() as u16;
        let close_w = PANE_CLOSE.chars().count() as u16;
        // The DIFF and MENU BUTTONS lead the row's buttons, on a pane
        // reading a card: the checkout's diff (`g`) and the card's own
        // menu, a right-click's. They, then the SIDE BUTTON, are what a
        // narrow pane gives up first; the FULL-SCREEN and CLOSE BUTTONS
        // stay while there is room for the two.
        let reads_card = pane_reads_card(app);
        let mut extras: Vec<(&'static str, HitTarget)> = Vec::new();
        if reads_card {
            extras.push((PANE_DIFF, HitTarget::Button(crate::keymap::Action::GitDiff)));
            extras.push((PANE_MENU, HitTarget::LauncherPaneMenu));
        }
        if let Some(glyph) = side_button {
            extras.push((glyph, HitTarget::LauncherPaneSide));
        }
        // What the title needs to say anything: a dot and a few letters.
        let title_min = 12u16;
        while !extras.is_empty() {
            let w: u16 = extras.iter().map(|(g, _)| g.chars().count() as u16).sum();
            if zoom_w + close_w + w + title_min <= r.width {
                break;
            }
            extras.remove(0);
        }
        let extras_w: u16 = extras.iter().map(|(g, _)| g.chars().count() as u16).sum();
        let buttons_w = zoom_w + extras_w + close_w;
        let taken = usize::from(buttons_w)
            + right
                .as_ref()
                .map_or(0, |tag| tag.content.chars().count() + 2);
        let tabs = pane_title(app, usize::from(r.width).saturating_sub(taken));
        let mut spans = Vec::new();
        let mut x = r.x;
        for tab in tabs {
            let width = u16::try_from(tab.width()).unwrap_or(u16::MAX);
            if let Some(hit) = tab.hit {
                app.chrome.hits.push((
                    Rect {
                        x,
                        y: r.y,
                        width,
                        height: 1,
                    },
                    hit,
                ));
            }
            x = x.saturating_add(width);
            spans.extend(tab.spans);
        }
        f.render_widget(Paragraph::new(Line::from(spans)), r);
        let tag_r = Rect {
            width: r.width.saturating_sub(buttons_w),
            ..r
        };
        if let Some(tag) = right {
            f.render_widget(
                Paragraph::new(Line::from(vec![tag, Span::raw(" ")]))
                    .alignment(ratatui::layout::Alignment::Right),
                tag_r,
            );
        }
        if r.width >= buttons_w {
            let mut x = tag_r.x + tag_r.width;
            for (glyph, hit) in extras
                .iter()
                .filter(|(_, hit)| *hit != HitTarget::LauncherPaneSide)
            {
                let w = glyph.chars().count() as u16;
                header_button(
                    f,
                    app,
                    Rect { x, width: w, ..r },
                    glyph,
                    hit.clone(),
                    th.accent,
                );
                x += w;
            }
            let side_button = extras
                .iter()
                .find(|(_, hit)| *hit == HitTarget::LauncherPaneSide)
                .map(|(glyph, _)| *glyph);
            let side_w = side_button.map_or(0, |b| b.chars().count() as u16);
            let zoom = Rect {
                x,
                width: zoom_w,
                ..r
            };
            header_button(
                f,
                app,
                zoom,
                PANE_FULL_SCREEN,
                HitTarget::LauncherPaneZoom,
                th.accent,
            );
            x += zoom_w;
            if let Some(glyph) = side_button {
                let side = Rect {
                    x,
                    width: side_w,
                    ..r
                };
                // Lit under the pointer as the header's other buttons are:
                // a half-filled square does not say it is one until then.
                let fg = if app.launcher.hover_crumb == Some(HitTarget::LauncherPaneSide) {
                    th.accent
                } else {
                    th.muted
                };
                f.render_widget(
                    Paragraph::new(Span::styled(glyph, Style::default().fg(fg))),
                    side,
                );
                app.chrome.hits.push((side, HitTarget::LauncherPaneSide));
                x += side_w;
            }
            let close = Rect {
                x,
                width: close_w,
                ..r
            };
            // Marked under the pointer the way the PROJECT TABS' `×` is:
            // nothing about a cross says it is a button until then.
            let fg = if app.launcher.hover_crumb == Some(HitTarget::LauncherPaneClose) {
                th.err
            } else {
                th.muted
            };
            f.render_widget(
                Paragraph::new(Span::styled(PANE_CLOSE, Style::default().fg(fg))),
                close,
            );
            app.chrome.hits.push((close, HitTarget::LauncherPaneClose));
        }
    }
    draw_rule(f, area, 2, if focused { th.accent } else { th.edge });
    Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(3),
        ..area
    }
}

/// The word a session's state goes by beside its STATUS DOT — the
/// desktop's words, so state never rests on color alone.
fn status_word(status: nebula_core::AgentStatus, unseen: bool) -> &'static str {
    use nebula_core::AgentStatus as S;
    match status {
        S::NeedsFeedback => "needs you",
        S::Running => "running",
        S::Finished | S::Terminated | S::Disconnected if unseen => "unread",
        S::Finished => "done",
        S::Terminated | S::Disconnected => "stopped",
        S::Fresh => "idle",
    }
}

/// What a session's PANE header says after its name — the desktop
/// session header's row of facts.
#[derive(Debug, Clone, PartialEq)]
struct PaneFacts {
    status: (&'static str, Color),
    /// Why its last turn stopped short, `resumes 3:45pm`, in its tone:
    /// it explains the status, so it comes right after it.
    stop: Option<(String, Color)>,
    /// `claude@max2 opus high`: [`runs_on_line`].
    runs_on: String,
    /// `⌂ ` or `↳ `, and `project / branch`, in the checkout's SCOPE COLOR.
    place: (&'static str, String, Color),
    /// `ctx 42% ⟳2`, in its tone.
    ctx: Option<(String, Color)>,
    /// `#12 ready`, in the pull request's own colors.
    pr: Option<(String, Color)>,
    /// How long ago the status last changed: `3m`.
    age: Option<String>,
    prompt: Option<String>,
}

fn pane_facts(app: &App, row: &LauncherRow, is_main: bool, now_ms: i64) -> PaneFacts {
    let th = app.chrome.theme;
    let a = &row.agent;
    let stop = stop_look(app, a, th);
    let ctx = app.launcher.session_contexts.get(&a.id).and_then(|c| {
        let label = crate::accounts_overlay::context_label(c)?;
        let fg = match nebula_core::usage::tone(c.percent?) {
            nebula_core::usage::Tone::Ok => th.muted,
            nebula_core::usage::Tone::Watch => th.accent,
            nebula_core::usage::Tone::Warn => th.warn,
            nebula_core::usage::Tone::Hot => th.err,
        };
        Some((label, fg))
    });
    let pr = row.pr.as_ref().map(|pr| {
        let look = crate::pr_row::look(pr.standing, pr.trouble, th);
        (format!("#{} {}", pr.number, pr.badge()), look.badge)
    });
    let age = (a.status_changed_at > 0).then(|| {
        crate::hosts::ago_label(now_ms - a.status_changed_at)
            .trim_end_matches(" ago")
            .to_string()
    });
    let (glyph, scope) = if is_main {
        ("⌂ ", th.root)
    } else {
        ("↳ ", th.worktree)
    };
    PaneFacts {
        status: (
            status_word(a.status, a.unseen),
            crate::ui::rows::status_color(Some(a.status), a.unseen, th),
        ),
        stop,
        runs_on: runs_on_line(a, &mut None),
        place: (glyph, format!("{} / {}", row.project, row.branch), scope),
        ctx,
        pr,
        age: age.filter(|s| !s.is_empty()),
        prompt: crate::launcher::last_prompt(a)
            .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|p| !p.is_empty()),
    }
}

/// The last prompt takes what the facts leave only from this many
/// columns up: a few letters of it say nothing.
const PROMPT_MIN: usize = 12;

/// [`PaneFacts`] laid out in `room` columns, most telling first: the
/// status word, why it stopped short, what it runs on, where, its
/// context, its pull request, its age — each whole or not at all, and
/// once one has not fitted nothing after it is tried, so a narrowing
/// pane only takes words away — then the last prompt, cut to whatever
/// is left.
#[cfg(test)]
fn fact_spans(facts: &PaneFacts, room: usize, th: Theme) -> Vec<Span<'static>> {
    fact_groups(facts, room, th)
        .into_iter()
        .flat_map(|(spans, _)| spans)
        .collect()
}

/// [`fact_spans`] in its pieces, each with its leading gap, the one
/// saying what the session runs on (`claude@max2 opus`) flagged: that
/// piece is the ACCOUNT CHIP, a button onto MOVE TO ACCOUNT.
fn fact_groups(facts: &PaneFacts, room: usize, th: Theme) -> Vec<(Vec<Span<'static>>, bool)> {
    const GAP: &str = "  ";
    // The pieces come back gap-first; regroup them at each gap.
    let mut groups: Vec<Vec<Span<'static>>> = Vec::new();
    for span in fact_pieces(facts, room, th) {
        if span.content == GAP || groups.is_empty() {
            groups.push(Vec::new());
        }
        if let Some(last) = groups.last_mut() {
            last.push(span);
        }
    }
    let mut chip_seen = false;
    groups
        .into_iter()
        .map(|g| {
            let chip = !chip_seen
                && g.len() == 2
                && g[0].content == GAP
                && g[1].content == facts.runs_on.as_str();
            chip_seen |= chip;
            (g, chip)
        })
        .collect()
}

/// Register the ACCOUNT CHIP's hit for session `id` over the runs-on
/// group, laid out from `x` on row `y`; the spans, flattened.
fn account_chip_hits(
    app: &mut App,
    groups: Vec<(Vec<Span<'static>>, bool)>,
    mut x: u16,
    y: u16,
    id: &nebula_core::AgentId,
) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let lit = app.launcher.hover_crumb.as_ref() == Some(&HitTarget::AccountChip(id.clone()));
    for (mut spans, chip) in groups {
        let width: usize = spans.iter().map(Span::width).sum();
        if chip {
            // The gap stays out of the target.
            let gap = spans.first().map_or(0, Span::width);
            app.chrome.hits.push((
                Rect {
                    x: x.saturating_add(gap as u16),
                    y,
                    width: (width - gap) as u16,
                    height: 1,
                },
                HitTarget::AccountChip(id.clone()),
            ));
            if lit {
                for s in spans.iter_mut().skip(1) {
                    s.style = s.style.add_modifier(Modifier::UNDERLINED);
                }
            }
        }
        x = x.saturating_add(width as u16);
        out.extend(spans);
    }
    out
}

fn fact_pieces(facts: &PaneFacts, room: usize, th: Theme) -> Vec<Span<'static>> {
    const GAP: &str = "  ";
    let dim = Style::default().fg(th.dim);
    let mut pieces: Vec<Vec<Span<'static>>> = vec![
        vec![Span::styled(
            facts.status.0,
            Style::default().fg(facts.status.1),
        )],
        vec![Span::styled(
            facts.runs_on.clone(),
            Style::default().fg(th.muted),
        )],
        vec![
            Span::styled(facts.place.0, Style::default().fg(facts.place.2)),
            Span::styled(facts.place.1.clone(), Style::default().fg(facts.place.2)),
        ],
    ];
    if let Some((label, fg)) = &facts.stop {
        pieces.insert(
            1,
            vec![Span::styled(label.clone(), Style::default().fg(*fg))],
        );
    }
    if let Some((label, fg)) = &facts.ctx {
        pieces.push(vec![Span::styled(label.clone(), Style::default().fg(*fg))]);
    }
    if let Some((label, fg)) = &facts.pr {
        pieces.push(vec![Span::styled(label.clone(), Style::default().fg(*fg))]);
    }
    if let Some(age) = &facts.age {
        pieces.push(vec![Span::styled(age.clone(), dim)]);
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut left = room;
    for piece in pieces {
        if piece.iter().all(|s| s.content.is_empty()) {
            continue;
        }
        let w = GAP.len() + piece.iter().map(Span::width).sum::<usize>();
        if w > left {
            return out;
        }
        left -= w;
        out.push(Span::raw(GAP));
        out.extend(piece);
    }
    if let Some(prompt) = &facts.prompt {
        let room = left.saturating_sub(GAP.len() + "› ".len());
        if room >= PROMPT_MIN {
            out.push(Span::raw(GAP));
            out.push(Span::styled("› ", dim));
            out.push(Span::styled(
                truncate(prompt, room),
                Style::default().fg(th.muted),
            ));
        }
    }
    out
}

/// The pane's title, laid out left to right in `room` columns: the card
/// the cursor is on — its STATUS DOT and name for a session, the
/// SESSIONS PANEL's `❯`/`▶` and name for a terminal — in the accent,
/// since it is what the keys reach; the checkout it runs in after it,
/// in the SCOPE COLOR the band's rule paints it. With the pane on
/// something the grid no longer lists — a card just archived out from
/// under it — the attached session's own name, muted.
fn pane_title(app: &App, room: usize) -> Vec<PaneTab> {
    let th = app.chrome.theme;
    let bands = crate::launcher::bands(app);
    let at = crate::launcher::cursor(app, &bands);
    let card = at.and_then(|at| crate::launcher::card_at(&bands, at));
    let name_room = (room / 2).max(8);
    let lit = Style::default().fg(th.accent).add_modifier(Modifier::BOLD);
    let mut tabs = Vec::new();
    match card {
        Some(crate::launcher::Card::Session(row)) => {
            let a = &row.agent;
            let name = truncate(&a.name, name_room);
            let mut used = 2 + name.chars().count();
            tabs.push(PaneTab::plain(vec![
                status_dot(Some(a.status), a.unseen, th),
                Span::styled(name, lit),
            ]));
            // Its SUBAGENTS, a button onto the SUBAGENTS OVERLAY.
            if let Some(spans) = super::subagents_view::pane_chip(app, &a.id) {
                let tab = PaneTab {
                    spans,
                    hit: Some(HitTarget::SubagentChip(a.id.clone())),
                };
                used += tab.width();
                tabs.push(tab);
            }
            // A session's header says what the desktop's does, as far as
            // the row allows ([`fact_spans`]); the checkout is one of the
            // facts, so the block below is the terminal's.
            let is_main = at
                .and_then(|at| bands.get(at.band))
                .is_some_and(|b| b.is_main);
            let facts = pane_facts(app, row, is_main, crate::app::now_ms());
            // What it runs on is the ACCOUNT CHIP: a click opens MOVE TO
            // ACCOUNT on the session.
            let chip = HitTarget::AccountChip(a.id.clone());
            let lit = app.launcher.hover_crumb.as_ref() == Some(&chip);
            for (mut spans, is_chip) in fact_groups(&facts, room.saturating_sub(used), th) {
                if !is_chip {
                    tabs.push(PaneTab::plain(spans));
                    continue;
                }
                let piece = spans.split_off(1);
                tabs.push(PaneTab::plain(spans));
                tabs.push(PaneTab {
                    spans: piece
                        .into_iter()
                        .map(|s| {
                            if lit {
                                s.patch_style(Style::default().add_modifier(Modifier::UNDERLINED))
                            } else {
                                s
                            }
                        })
                        .collect(),
                    hit: Some(chip.clone()),
                });
            }
            return tabs;
        }
        Some(crate::launcher::Card::Terminal(t)) => {
            let glyph = if t.run_command.is_some() {
                "▶ "
            } else {
                "❯ "
            };
            tabs.push(PaneTab::plain(vec![
                Span::styled(
                    glyph,
                    Style::default().fg(if t.alive { th.ok } else { th.dim }),
                ),
                Span::styled(truncate(&t.name, name_room), lit),
            ]));
        }
        None => {
            if let Some(name) = app
                .pane
                .term
                .as_ref()
                .and_then(|t| attached_name(app, &t.sref))
            {
                tabs.push(PaneTab::plain(vec![Span::styled(
                    truncate(&name, name_room),
                    Style::default().fg(th.muted),
                )]));
            }
        }
    }
    // The checkout everything in the pane belongs to, in the SCOPE COLOR
    // the band's rule paints it — `⌂` on the project's root branch, `↳`
    // on a checkout of its own — so the pane and the grid say the same
    // thing the same way.
    let checkout = at
        .and_then(|at| bands.get(at.band))
        .map(|b| (b.is_main, b.branch.clone()))
        .or_else(|| {
            app.selected_worktree()
                .map(|w| (w.is_main, w.branch.clone()))
        });
    if let Some((is_main, branch)) = checkout {
        let (glyph, scope) = if is_main {
            ("⌂ ", th.root)
        } else {
            ("↳ ", th.worktree)
        };
        if !tabs.is_empty() {
            tabs.push(PaneTab::plain(vec![Span::raw("  ")]));
        }
        tabs.push(PaneTab::plain(vec![
            Span::styled(glyph, Style::default().fg(scope)),
            Span::styled(
                truncate(&branch, (room / 4).max(8)),
                Style::default().fg(scope),
            ),
        ]));
    }
    tabs
}

/// The name of the session `sref` attaches, whichever kind it is.
fn attached_name(app: &App, sref: &nebula_core::SessionRef) -> Option<String> {
    match sref {
        nebula_core::SessionRef::Agent(id) => app
            .tree
            .agents
            .iter()
            .find(|a| &a.id == id)
            .map(|a| a.name.clone()),
        nebula_core::SessionRef::Terminal(id) => app
            .tree
            .terminals
            .iter()
            .find(|t| &t.id == id)
            .map(|t| t.name.clone()),
    }
}

/// The full-screen session's own header, in place of the pane's
/// `TERMINAL · name`: a breadcrumb back to the grid — `‹ sessions` is a
/// button, the session's name the crumb it leads out of — with the
/// harness it runs on and where, right-aligned before the NORMAL-SIZE
/// BUTTON. Returns the rect the PTY
/// draws in, as `terminal_frame` does.
pub(super) fn crumb_frame(f: &mut Frame, app: &mut App, area: Rect) -> Rect {
    let th = app.chrome.theme;
    if let Some(r) = row_rect(area, 1) {
        let r = pad_x(r);
        // The NORMAL-SIZE BUTTON holds the right end of the row, where the
        // pane's header keeps its buttons; the rest is laid out in what it
        // leaves.
        let size_w = PANE_NORMAL_SIZE.chars().count() as u16;
        if r.width > size_w {
            let size = Rect {
                x: r.x + r.width - size_w,
                width: size_w,
                ..r
            };
            header_button(
                f,
                app,
                size,
                PANE_NORMAL_SIZE,
                HitTarget::LauncherPaneZoom,
                th.accent,
            );
        }
        let r = Rect {
            width: r.width.saturating_sub(size_w),
            ..r
        };
        let back = format!("‹ {CRUMB}");
        // The hatch out of a full-screen session is a button too, and
        // wears the same underline while the pointer is on it.
        let mut style = Style::default().fg(th.accent).add_modifier(Modifier::BOLD);
        if app.launcher.hover_crumb.as_ref() == Some(&HitTarget::LauncherCrumb) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        let mut spans = vec![Span::styled(back.clone(), style)];
        // Only the crumb itself is the button — a click anywhere else on
        // the header row belongs to the pane.
        app.chrome.hits.push((
            Rect {
                width: back.chars().count() as u16,
                height: 1,
                ..r
            },
            HitTarget::LauncherCrumb,
        ));
        let row = app
            .pane
            .term
            .as_ref()
            .map(|t| t.sref.clone())
            .and_then(|sref| match sref {
                nebula_core::SessionRef::Agent(id) => crate::launcher::row(app, &id),
                _ => None,
            });
        if let Some(row) = &row {
            let a = &row.agent;
            spans.push(Span::styled(" / ", Style::default().fg(th.dim)));
            spans.push(status_dot(Some(a.status), a.unseen, th));
            spans.push(Span::styled(
                truncate(&a.name, usize::from(r.width / 2).max(8)),
                Style::default().fg(th.text).add_modifier(Modifier::BOLD),
            ));
            // The pane header's facts, in what the crumb leaves: the
            // state, what it runs on and where, its context, pull
            // request, age and last prompt, as far as the row allows.
            let used: usize = spans.iter().map(Span::width).sum();
            let is_main = app
                .tree
                .worktrees
                .iter()
                .any(|w| w.id == a.worktree_id && w.is_main);
            let facts = pane_facts(app, row, is_main, crate::app::now_ms());
            let groups = fact_groups(&facts, usize::from(r.width).saturating_sub(used), th);
            let id = a.id.clone();
            spans.extend(account_chip_hits(
                app,
                groups,
                r.x.saturating_add(used as u16),
                r.y,
                &id,
            ));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }
    let focused = app.nav.focus == Focus::Terminal;
    draw_rule(f, area, 2, if focused { th.accent } else { th.edge });
    Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(3),
        ..area
    }
}

/// The view's box, row 0: where the session runs — the project and the
/// checkout in it — what runs there and on which model, each named with
/// the chord that changes it, set far enough apart that no two read as
/// one phrase. The checkout's chord is `^T`: it, or a click on the
/// branch, drops the WORKTREE PICKER down from it.
///
/// Widest form that fits, in order: labelled and airy; labelled and
/// tight; the values and their chords alone; then the same without the
/// model, without the agent, and without the worktree. The airy form is
/// drawn whole or not at all — tighter gaps beat a cut name. The project
/// is never dropped, only cut — where a session lands is the one thing
/// worth a whole row on its own — and a long branch is cut before it is.
///
/// The branch wears the SCOPE COLOR the grid paints its checkout in —
/// `root` on the project's root branch, `worktree` on one of its own —
/// except a fresh worktree's, the one Enter will cut, drawn in the green
/// the box's frame turns. A box aimed away from the grid behind it
/// fires a BACKGROUND LAUNCH: the project is the half that changed, and
/// nothing on screen will move when Enter lands, so the project is lit.
/// A PR SESSION's head branch wears no `^T` and is no button: its
/// checkout is the DAEMON's to pick.
///
/// Returns the row, each field's own columns within it — the label, the
/// value and the chord together, never the air between two fields — so a
/// click there can be given the same picker the chord opens
/// ([`crate::app::PromptDialog::detail_areas`]), and the branch's own, for
/// the picker to hang from ([`crate::app::PromptDialog::branch_area`]). A
/// tier that drops a field hands back no columns for it: what is not drawn
/// is not a button.
pub(super) fn detail_line(app: &App, launch: &QuickLaunch, width: u16, th: Theme) -> DetailLine {
    let details = Details::of(app, launch);
    let width = usize::from(width);
    // The PROFILE field rides last, and only while the harness has
    // profiles: it is the first to go for room.
    let all = 4 + usize::from(details.profile.is_some());
    for (labels, gap, fields, cuts) in [
        (true, DETAIL_GAP, all, false),
        (true, DETAIL_TIGHT, all, true),
        (false, DETAIL_TIGHT, all, true),
        (false, DETAIL_TIGHT, 4, true),
        (false, DETAIL_TIGHT, 3, true),
        (false, DETAIL_TIGHT, 2, true),
        (false, DETAIL_TIGHT, 1, true),
    ] {
        let drawn = details.spans(fields, labels, gap, th);
        let w = drawn.line.width();
        if w <= width {
            return drawn;
        }
        // This tier with the branch, then the project, cut to what is left
        // over — but not past the point where either stops saying which
        // one it is.
        if let Some(cut) = details.cut(w - width, fields).filter(|_| cuts) {
            return cut.spans(fields, labels, gap, th);
        }
    }
    // Narrower than the project and its chord together: cut it to the row.
    let cut = Details {
        project: truncate(&details.project, width.saturating_sub(3)),
        ..details
    };
    cut.spans(1, false, DETAIL_TIGHT, th)
}

/// [`detail_line`]'s answer: the row, each drawn field's `(field, first
/// column, width)` within it, and the branch's `(first column, width)` —
/// the value and its `^T`, None when it was dropped for room or names a
/// checkout nobody picks.
pub(super) struct DetailLine {
    pub line: Line<'static>,
    pub fields: Vec<(BoxField, u16, u16)>,
    pub branch: Option<(u16, u16)>,
}

/// Shortest a project name is cut to before [`detail_line`] gives up a
/// whole field instead.
const MIN_PROJECT: usize = 8;

/// Shortest a branch is cut to before [`detail_line`] starts on the
/// project.
const MIN_BRANCH: usize = 12;

/// The gap between two of [`detail_line`]'s fields — wide enough that
/// `api-server ^P` and `harness claude` never read as one phrase — and the
/// one a box too narrow for that falls back to.
const DETAIL_GAP: &str = "   ·   ";
const DETAIL_TIGHT: &str = " · ";

/// What [`detail_line`] names, before a tier decides how much of it fits.
#[derive(Clone)]
struct Details {
    project: String,
    branch: String,
    harness: String,
    model: String,
    /// The PROFILE the launch runs under — its name, or `none`, with the
    /// account's last PLAN USAGE beside it when one is known — or None
    /// when the harness has no profiles, and no field is drawn.
    profile: Option<String>,
    /// How used the profile's account is: the field goes ember at 80%,
    /// red at 95%.
    profile_tone: nebula_core::usage::Tone,
    /// The branch is the WORKTREE PICKER's button — anything but a PR
    /// SESSION's head.
    pickable: bool,
    /// Enter cuts the branch as a fresh worktree first.
    fresh: bool,
    /// The branch is the project's ROOT WORKTREE.
    root: bool,
    /// The box is aimed away from the grid behind it.
    background: bool,
}

impl Details {
    fn of(app: &App, launch: &QuickLaunch) -> Self {
        let mut harness = launch
            .custom
            .as_deref()
            .unwrap_or_else(|| launch.kind.as_str())
            .to_string();
        // A CLAUDE CLOUD box says so on the button that toggles it.
        if launch.cloud {
            harness.push_str(" · cloud");
        }
        let mut model = launch
            .model
            .clone()
            .unwrap_or_else(|| crate::config::DEFAULT_CHOICE.into());
        if let Some(effort) = launch.effort.as_deref().filter(|e| !e.is_empty()) {
            model.push(' ');
            model.push_str(effort);
        }
        let branch = match &launch.pr {
            Some(pr) => pr.head.clone(),
            None => crate::quick_prompt::target_branch(app, launch)
                .unwrap_or_else(|| "(worktree gone)".into()),
        };
        let (profiles, policy) = (&app.launcher.profiles, app.launcher.spread);
        let harness_id = launch.harness_id();
        let project = crate::quick_prompt::project_default(app, &launch.target, harness_id);
        let project = project.as_deref();
        let profile = (!launch.profile_choices(profiles, policy, project).is_empty()).then(|| {
            let name = crate::quick_prompt::profile_choice_label(
                launch.profile.as_deref(),
                profiles,
                harness_id,
                policy,
                &app.launcher.account_usage,
                project,
            );
            // The bars are the account Enter lands on: "auto"'s pick.
            let on = crate::quick_prompt::resolved_profile(
                launch.profile.as_deref(),
                profiles,
                harness_id,
                policy,
                &app.launcher.account_usage,
                project,
            );
            let key = nebula_core::usage::account_key(on.as_deref(), harness_id);
            let reading = app.launcher.account_usage.iter().find(|u| u.account == key);
            let now = nebula_core::clock::now_secs() as i64;
            (
                crate::accounts_overlay::field_text(&name, reading, now),
                crate::accounts_overlay::reading_tone(reading, now),
            )
        });
        let (profile, profile_tone) = match profile {
            Some((text, tone)) => (Some(text), tone),
            None => (None, nebula_core::usage::Tone::Ok),
        };
        Self {
            project: launch_project(app, launch),
            branch,
            harness,
            model,
            profile,
            profile_tone,
            pickable: launch.pr.is_none(),
            fresh: launch.is_new_worktree(),
            root: launch.pr.is_none()
                && matches!(&launch.target, crate::quick_prompt::QuickTarget::Worktree(id)
                    if app.tree.worktrees.iter().any(|w| &w.id == id && w.is_main)),
            background: crate::launcher::project_of(app, &launch.target)
                .is_some_and(|project| crate::launcher::is_background(app, &project)),
        }
    }

    /// These details `over` columns narrower: taken from the branch while
    /// the tier draws it, down to [`MIN_BRANCH`], then from the project,
    /// down to [`MIN_PROJECT`]. None when that is not enough.
    fn cut(&self, over: usize, fields: usize) -> Option<Self> {
        let branch = self.branch.chars().count();
        let from_branch = if fields >= 2 {
            branch.saturating_sub(MIN_BRANCH).min(over)
        } else {
            0
        };
        let from_project = over - from_branch;
        let project = self.project.chars().count();
        if from_project > 0 && project.saturating_sub(from_project) < MIN_PROJECT {
            return None;
        }
        Some(Self {
            project: truncate(&self.project, project - from_project),
            branch: truncate(&self.branch, branch - from_branch),
            ..self.clone()
        })
    }

    /// The row at one tier: `fields` of them, each a `value ^key` with the
    /// word for it ahead when `labels`, held apart by `gap`. Each field's
    /// columns are measured as its spans are pushed, so a name's own width
    /// is what the button is worth.
    fn spans(&self, fields: usize, labels: bool, gap: &str, th: Theme) -> DetailLine {
        let bold = |fg| Style::default().fg(fg).add_modifier(Modifier::BOLD);
        let mut spans = Vec::new();
        let mut hits = Vec::new();
        let mut branch = None;
        let mut x = 0usize;
        let push = |spans: &mut Vec<Span<'static>>, x: &mut usize, span: Span<'static>| {
            *x += span.width();
            spans.push(span);
        };
        let project_fg = if self.background { th.accent } else { th.text };
        let branch_fg = if self.fresh {
            th.ok
        } else if self.root {
            th.root
        } else {
            th.worktree
        };
        for (i, (field, label, value, fg, key)) in [
            (
                BoxField::Project,
                "project",
                &self.project,
                project_fg,
                Some("^P"),
            ),
            (
                BoxField::Worktree,
                "worktree",
                &self.branch,
                branch_fg,
                self.pickable.then_some("^T"),
            ),
            (
                BoxField::Agent,
                "harness",
                &self.harness,
                th.text,
                Some("Tab"),
            ),
            (BoxField::Model, "model", &self.model, th.text, Some("^O")),
            (
                BoxField::Profile,
                "profile",
                self.profile.as_ref().unwrap_or(&self.model),
                match self.profile_tone {
                    nebula_core::usage::Tone::Ok => th.text,
                    nebula_core::usage::Tone::Watch => th.accent,
                    nebula_core::usage::Tone::Warn => th.warn,
                    nebula_core::usage::Tone::Hot => th.err,
                },
                Some("^A"),
            ),
        ]
        .into_iter()
        .take(fields)
        .enumerate()
        {
            if i > 0 {
                push(
                    &mut spans,
                    &mut x,
                    Span::styled(gap.to_string(), Style::default().fg(th.edge)),
                );
            }
            let start = x;
            if labels {
                push(
                    &mut spans,
                    &mut x,
                    Span::styled(format!("{label} "), Style::default().fg(th.dim)),
                );
            }
            let value_at = x;
            push(&mut spans, &mut x, Span::styled(value.clone(), bold(fg)));
            if let Some(key) = key {
                push(
                    &mut spans,
                    &mut x,
                    Span::styled(format!(" {key}"), Style::default().fg(th.accent)),
                );
            }
            if field == BoxField::Worktree {
                if !self.pickable {
                    continue;
                }
                branch = Some((value_at as u16, (x - value_at) as u16));
            }
            hits.push((field, start as u16, (x - start) as u16));
        }
        DetailLine {
            line: Line::from(spans),
            fields: hits,
            branch,
        }
    }
}

/// The PROJECT a launch is aimed at, by name.
fn launch_project(app: &App, launch: &QuickLaunch) -> String {
    crate::launcher::project_of(app, &launch.target)
        .and_then(|id| crate::launcher::project_name(app, &id))
        .unwrap_or_else(|| "(project gone)".into())
}

/// The view's box prompt header: what Enter sends on the left, and the
/// toggle that cuts a fresh worktree, right. A PR SESSION says what the
/// DAEMON will do with the head branch instead, its checkout not being
/// ours to flip. Where the launch lands — the project and the checkout in
/// it — is on the details row above ([`detail_line`]).
///
/// Returns the line and the toggle's columns within it, so a click there
/// can be given the same flip `^N` has
/// ([`crate::app::PromptDialog::toggle_area`]).
pub(super) fn target_line(launch: &QuickLaunch, label: &str, width: u16, th: Theme) -> TargetLine {
    let dim = Style::default().fg(th.dim);
    // Right: the toggle and its state, or — on a PR SESSION — what the
    // DAEMON will do with the head branch, there being nothing to flip.
    let (right, clickable) = if launch.pr.is_some() {
        (vec![Span::styled("reused or cut on Enter", dim)], false)
    } else if launch.is_new_worktree() {
        let on = Style::default().fg(th.ok).add_modifier(Modifier::BOLD);
        (
            vec![
                Span::styled("[✓] new worktree", on),
                Span::styled(" ^N", Style::default().fg(th.ok)),
            ],
            true,
        )
    } else {
        (
            vec![
                Span::styled("[ ] new worktree", dim),
                Span::styled(" ^N", dim),
            ],
            true,
        )
    };
    let right_w: usize = right.iter().map(|s| s.width()).sum();

    let width = usize::from(width);
    let label = truncate(label, width.saturating_sub(right_w + 1));
    let used = label.chars().count();
    let mut spans = vec![Span::styled(label, dim)];
    // The right half goes whole or not at all, as the panels' box's does.
    let mut toggle = None;
    if width > used + right_w {
        let pad = width - used - right_w;
        spans.push(Span::raw(" ".repeat(pad)));
        if clickable {
            toggle = Some(((used + pad) as u16, right_w as u16));
        }
        spans.extend(right);
    }
    TargetLine {
        line: Line::from(spans),
        toggle,
    }
}

/// [`target_line`]'s answer: the row, and where its toggle landed in it —
/// `(first column, width)`, None when it was dropped for room or has
/// nothing to do.
pub(super) struct TargetLine {
    pub line: Line<'static>,
    pub toggle: Option<(u16, u16)>,
}

/// The view's box title: what Enter starts. The harness, the model and
/// the effort are on [`detail_line`] under it, beside the chords that
/// change them; what is left here is what the box is *for* — an issue, a
/// pull request, an AGENT PRESET — as the panels' box names them.
pub(super) fn box_title(launch: &QuickLaunch) -> String {
    let mut head = vec!["New session".to_string()];
    if let Some(issue) = &launch.issue {
        head.push(format!("issue #{}", issue.number));
    }
    if let Some(pr) = &launch.pr {
        head.push(format!("PR #{}", pr.number));
    }
    if let Some(preset) = &launch.preset {
        head.push(preset.name.clone());
    }
    if launch.cloud {
        head.push("Claude Cloud".into());
    }
    head.join(" · ")
}

/// The view's box hints, widest that fits in `width`. `^P`, `Tab`, `^O`
/// and `^N` are not here: each one is now inside the box beside the thing
/// it changes, and a second copy along the border was most of what made
/// this box read as a wall of text.
pub(super) fn box_hint(width: u16) -> &'static str {
    if width >= 60 {
        " Enter launch · ⇧Enter newline · ⇧Tab preset · Esc cancel "
    } else if width >= 43 {
        " Enter launch · ⇧Tab preset · Esc cancel "
    } else if width >= 29 {
        " Enter launch · Esc cancel "
    } else if width >= 25 {
        " ↵ launch · Esc cancel "
    } else {
        " ↵ · Esc "
    }
}

/// Where the view's QUICK PROMPT is drawn — one place, so the PROJECT
/// PICKER can float over exactly the rect the box is in.
pub(super) fn box_rect(frame: Rect) -> Rect {
    centered_rect(frame, BOX_SIZE.0, BOX_SIZE.1)
}

/// Does the PROJECT PICKER float over the box, rather than stand on its
/// own in the middle of the screen? Only when a box was up to come back
/// to.
pub(super) fn picker_over_box(_app: &App, picker: &ProjectPicker) -> bool {
    picker.back.from_box
}

/// Where the PROJECT PICKER goes, sized to the list it has to show.
/// Opened from the box, it floats over it: inset inside the box's rect,
/// so the box's frame, its title and its details row stay on screen
/// around the list. The list gives up the rows that costs — the box
/// behind is worth more than four more projects. With no box under it,
/// it is centered on the screen as any other modal.
fn picker_rect(frame: Rect, over: Option<Rect>, matches: usize) -> Rect {
    // Unlike the menus that float over the box, the picker shrinks to fit
    // inside it rather than spilling back out to the middle of the
    // screen: a list has rows to give up, and the box behind is worth
    // more than four more projects.
    let max_rows = over
        .map(|b| b.height.saturating_sub(OVER_BOX_INSET + 4))
        .unwrap_or(PICKER_ROWS)
        .clamp(1, PICKER_ROWS);
    let rows = (matches as u16).clamp(1, max_rows);
    let width = over.map_or(PICKER_W, |b| {
        PICKER_W.min(b.width.saturating_sub(OVER_BOX_INSET))
    });
    over_box_rect(frame, over, width, rows + 4)
}

/// The PROJECT PICKER: the query on top, the projects under it — the name
/// with the typed letters lit, the path dim after it. Drawn over the box it was opened from
/// (`ui::draw_overlay` puts that box down first), inset inside it.
pub(super) fn draw_project_picker(f: &mut Frame, app: &mut App, picker: &ProjectPicker) {
    let th = app.chrome.theme;
    let over = picker_over_box(app, picker).then(|| box_rect(f.area()));
    let area = picker_rect(f.area(), over, picker.matches.len());
    let title = if picker.query.is_empty() {
        " Project ".to_string()
    } else {
        format!(
            " Project ({}/{}) ",
            picker.matches.len(),
            picker.projects.len()
        )
    };
    let inner = render_modal_frame(f, area, title, th);
    if let Some(query_area) = row_rect(inner, 0) {
        let line = search_line(&picker.query, "type a project name…", query_area, th);
        f.render_widget(Paragraph::new(line), query_area);
    }
    let list = below_first_row(inner);
    if picker.matches.is_empty() {
        empty_list_row(f, list, NO_MATCHES, th);
    }
    let selected = picker.selected.min(picker.matches.len().saturating_sub(1));
    let start = crate::app::window_start(selected, list.height as usize);
    for (row, (i, (index, positions))) in picker.matches.iter().enumerate().skip(start).enumerate()
    {
        let Some(row_area) = row_rect(list, row) else {
            break;
        };
        let project = &picker.projects[*index];
        let budget = (list.width as usize).saturating_sub(4);
        let name = truncate(&project.name, budget);
        let lit = visible_positions(positions, &name, &project.name);
        let mut spans = vec![Span::styled("▪ ", Style::default().fg(th.accent))];
        spans.extend(fuzzy_highlight_spans(&name, lit, th));
        let used = name.chars().count();
        let after = format!("  {}", project.path);
        if used + 2 < budget {
            spans.push(Span::styled(
                truncate(&after, budget - used),
                Style::default().fg(th.dim),
            ));
        }
        render_row(f, row_area, spans, i == selected, true, th);
    }
    if let Some(Overlay::ProjectPicker(p)) = &mut app.modals.overlay {
        p.area = area;
        p.list_area = list;
        p.selected = selected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`draw_card`] for the tests that draw one card alone.
    fn draw_one(
        f: &mut Frame,
        app: &App,
        area: Rect,
        row: &LauncherRow,
        selected: bool,
        focused: bool,
        th: Theme,
    ) {
        draw_card(
            f.buffer_mut(),
            app,
            area,
            row,
            selected,
            focused,
            th,
            &mut None,
        );
    }

    /// `^P` opens the PROJECT PICKER *over* the box, so the rect it takes
    /// is strictly inside the box's on all four sides — the frame, the
    /// title and the details row above the list stay on screen however
    /// long the project list is. With no box under it the picker is
    /// centered on the screen, as it always was.
    #[test]
    fn the_picker_floats_inside_the_box_it_was_opened_from() {
        let frame = Rect::new(0, 0, 130, 34);
        let boxed = box_rect(frame);
        for matches in [0usize, 1, 3, 200] {
            let area = picker_rect(frame, Some(boxed), matches);
            assert!(
                area.x > boxed.x && area.right() < boxed.right(),
                "{matches}: {area:?} is not inside {boxed:?} sideways"
            );
            assert!(
                area.y > boxed.y && area.bottom() < boxed.bottom(),
                "{matches}: {area:?} is not inside {boxed:?} top to bottom"
            );
        }
        // Long list, no box: the screen's middle and the full height.
        let loose = picker_rect(frame, None, 200);
        assert_eq!(loose.height, PICKER_ROWS + 4);
        assert_eq!(loose, centered_rect(frame, PICKER_W, PICKER_ROWS + 4));
    }

    /// A terminal too small for the box to float anything inside still
    /// gets a picker — clamped, never a zero-sized or off-screen rect.
    #[test]
    fn a_tiny_screen_still_draws_the_picker() {
        for (w, h) in [(20u16, 6u16), (30, 9), (60, 12)] {
            let frame = Rect::new(0, 0, w, h);
            let area = picker_rect(frame, Some(box_rect(frame)), 50);
            assert!(area.width > 0 && area.height > 0, "{w}x{h}: {area:?}");
            assert!(
                area.right() <= frame.right() && area.bottom() <= frame.bottom(),
                "{w}x{h}: {area:?} runs off {frame:?}"
            );
        }
    }

    /// Every tier of the box's hint fits the border it is drawn in.
    #[test]
    fn every_box_hint_fits_its_border() {
        for width in 11..=120u16 {
            let hint = box_hint(width);
            assert!(
                hint.chars().count() + 2 <= width as usize,
                "{width}: {hint:?}"
            );
        }
        // The chords the box used to repeat along its border now live
        // beside what they change, so the hint must not name them again.
        for width in 11..=120u16 {
            let hint = box_hint(width);
            for chord in ["^P", "^T", "^O", "^N", "Tab agent"] {
                assert!(
                    !hint.contains(chord),
                    "{width}: {hint:?} still says {chord}"
                );
            }
        }
        // And the box at its own size names every key it has.
        let full = box_hint(BOX_SIZE.0);
        for key in ["Enter launch", "⇧Enter newline", "⇧Tab preset"] {
            assert!(full.contains(key), "{full:?} lost {key}");
        }
    }

    fn a_launch() -> QuickLaunch {
        let cfg = crate::config::Config::default();
        let target =
            crate::quick_prompt::QuickTarget::Worktree(nebula_core::WorktreeId("w".into()));
        QuickLaunch::of_kind(
            target,
            nebula_core::AgentKind::Claude,
            None,
            None,
            None,
            &cfg,
        )
    }

    fn text_of(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// The title is what the box is *for*; the harness, the model and the
    /// effort moved down beside the chords that change them.
    #[test]
    fn the_box_title_is_what_the_box_is_for() {
        let mut launch = a_launch();
        launch.model = None;
        launch.effort = None;
        assert_eq!(box_title(&launch), "New session");
        launch.model = Some("opus".into());
        launch.effort = Some("high".into());
        assert_eq!(box_title(&launch), "New session");
    }

    /// The details row names all four values and their four chords, and
    /// fits the box it is drawn in.
    #[test]
    fn the_details_row_carries_every_chord() {
        let th = Theme::default();
        let app = App::new();
        let mut launch = a_launch();
        launch.model = None;
        launch.effort = None;
        let inner = BOX_SIZE.0 - 4;
        let line = detail_line(&app, &launch, inner, th).line;
        let text = text_of(&line);
        for want in [
            "project ",
            "^P",
            "worktree ",
            "^T",
            "harness ",
            "claude",
            "Tab",
            "model ",
            "default",
            "^O",
        ] {
            assert!(text.contains(want), "{text:?} is missing {want:?}");
        }
        assert!(line.width() <= inner as usize, "{text:?}");

        launch.model = Some("opus".into());
        launch.effort = Some("high".into());
        assert!(
            text_of(&detail_line(&app, &launch, inner, th).line).contains("opus high"),
            "the effort belongs on the model"
        );

        // One column short of the airy form: the gaps tighten and every
        // name stays whole, rather than one name losing its last letter.
        let airy = detail_line(&app, &launch, 400, th).line.width() as u16;
        let text = text_of(&detail_line(&app, &launch, airy - 1, th).line);
        assert!(!text.contains('…'), "{text:?}");
        assert!(text.contains(" · worktree "), "{text:?}");
    }

    /// Every field the row draws hands back the columns it was drawn in,
    /// and those columns hold exactly that field's own text — so a click
    /// on `harness claude Tab` cannot open the model list. A tier that drops
    /// a field hands back nothing for it: what is not drawn is no button.
    #[test]
    fn every_drawn_detail_hands_back_its_own_columns() {
        let th = Theme::default();
        let app = App::new();
        let mut launch = a_launch();
        launch.model = Some("opus".into());
        launch.effort = Some("high".into());
        for width in 10..=120u16 {
            let DetailLine {
                line, fields: hits, ..
            } = detail_line(&app, &launch, width, th);
            let text: Vec<char> = text_of(&line).chars().collect();
            assert!(
                hits.iter().any(|(f, _, _)| *f == BoxField::Project),
                "{width}: the project is never dropped"
            );
            for (field, x, w) in &hits {
                let (x, w) = (*x as usize, *w as usize);
                assert!(x + w <= text.len(), "{width}: {field:?} runs past the row");
                let cut: String = text[x..x + w].iter().collect();
                let chord = match field {
                    BoxField::Project => "^P",
                    BoxField::Worktree => "^T",
                    BoxField::Agent => "Tab",
                    BoxField::Model => "^O",
                    BoxField::Profile => "^A",
                };
                assert!(
                    cut.ends_with(chord),
                    "{width}: {field:?} is {cut:?}, which does not end in {chord}"
                );
                assert!(
                    !cut.starts_with(' ') && !cut.contains('·'),
                    "{width}: {field:?} is {cut:?}, which reaches into the air beside it"
                );
            }
        }
    }

    /// However narrow the box gets, the details row fits inside it and
    /// still says which project the launch is aimed at — it gives up a
    /// whole field before it clips a name in half.
    #[test]
    fn the_details_row_fits_every_width() {
        let th = Theme::default();
        let app = App::new();
        let mut launch = a_launch();
        launch.model = Some("opus".into());
        launch.effort = Some("high".into());
        for width in 10..=120u16 {
            let line = detail_line(&app, &launch, width, th).line;
            let text = text_of(&line);
            assert!(line.width() <= width as usize, "{width}: {text:?}");
            assert!(text.contains("^P"), "{width}: {text:?}");
        }
    }

    /// Two projects, one session in each: `api` with a `feat` checkout,
    /// `web` with one of its own. Both sessions start idle, so nothing
    /// sweeps until a test says it does.
    fn a_tree() -> App {
        use nebula_core::{
            Agent, AgentId, AgentKind, AgentStatus, Project, ProjectId, Worktree, WorktreeId,
        };
        let mut app = App::new();
        app.tree.projects = ["api", "web"]
            .iter()
            .enumerate()
            .map(|(i, name)| Project {
                id: ProjectId(format!("p{i}")),
                name: (*name).into(),
                repo_path: format!("/tmp/{name}").into(),
                sort_order: 0,
            })
            .collect();
        app.tree.worktrees = (0..2)
            .map(|i| Worktree {
                id: WorktreeId(format!("w{i}")),
                project_id: ProjectId(format!("p{i}")),
                path: format!("/tmp/w{i}").into(),
                branch: "feat".into(),
                is_main: false,
                sort_order: 0,
            })
            .collect();
        app.tree.agents = (0..2)
            .map(|i| Agent {
                parent: None,
                profile: None,
                id: AgentId(format!("a{i}")),
                worktree_id: WorktreeId(format!("w{i}")),
                name: format!("s{i}"),
                status: AgentStatus::Finished,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: true,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            })
            .collect();
        app
    }

    /// `a_tree` with `count` sessions in `api` instead of one, so a grid
    /// can be given more cards than the body has room for.
    fn a_crowded_tree(count: usize) -> App {
        use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, WorktreeId};
        let mut app = a_tree();
        let one = app.tree.agents[0].clone();
        app.tree.agents = (0..count)
            .map(|i| Agent {
                parent: None,
                id: AgentId(format!("api{i}")),
                worktree_id: WorktreeId("w0".into()),
                name: format!("session {i}"),
                status: AgentStatus::Finished,
                kind: AgentKind::Claude,
                ..one.clone()
            })
            .collect();
        app
    }

    /// Aim the SESSIONS cursor at the session with this id, whichever
    /// row of the panels' list it happens to be on.
    fn aim_at(app: &mut App, id: &str) {
        app.nav.sel_session = app
            .visible_session_rows()
            .iter()
            .position(|row| matches!(row, crate::app::SessionRow::Agent(a) if a.id.0 == id))
            .expect("a row for the session");
    }

    /// The whole view, drawn with its one band open on the ACCORDION: a
    /// body too short for every card says so on the worktree's rule, and
    /// says it about exactly the cards the grid left off. This is what a
    /// PANE dragged up over the grid looks like — the cards that lost
    /// their room are counted rather than simply gone — and the grid's
    /// own edge says it again where the eye looks for the rest.
    #[test]
    fn a_body_too_short_for_its_cards_counts_them_in_the_header() {
        use crate::launcher::{BAND_RULE_H, BELOW_MARK_H, CARD_H, GAP_Y, HEAD_H};
        let mut app = a_crowded_tree(9);
        select(&mut app, "api");
        app.launcher.launcher_expanded = Some(nebula_core::WorktreeId("w0".into()));
        // The cursor on the grid's first card, the way a project opens:
        // everything missing is under the fold.
        let first = crate::launcher::rows(&app)[0].agent.id.0.clone();
        aim_at(&mut app, &first);
        // One column, the `sessions` rule, room for two rows of cards and
        // the row under them the `↓` marker takes: seven of the nine are
        // under the fold.
        let body = Rect::new(
            0,
            0,
            60,
            HEAD_H + BAND_RULE_H + CARD_H * 2 + GAP_Y + BELOW_MARK_H,
        );
        let lines = drawn_lines(&mut app, body);
        let head = &lines[1];
        assert!(head.contains("9 sessions"), "{head:?}");
        assert!(head.contains("↓ 7 hidden"), "{head:?}");

        // And the grid really did draw two of them: the header is not
        // guessing at a number the drawing disagrees with.
        let drawn = lines
            .iter()
            .filter(|line| line.contains("session "))
            .count();
        assert_eq!(drawn, 2, "two cards on screen, seven hidden: {lines:#?}");

        // The EDGE MARKERS: the row under the cards says what is below;
        // nothing is scrolled off the top, so the row of air over them
        // says nothing.
        let last = lines.last().unwrap();
        assert!(last.contains("↓ 7 more below"), "{last:?}");
        let air = &lines[usize::from(HEAD_H) - 1];
        assert!(air.trim().is_empty(), "nothing above: {air:?}");
    }

    /// The screenshot: nine sessions and three terminals, the cursor
    /// walked down onto the first terminal in a body two rows short of
    /// the lot. The window scrolls two rows, and the first row of
    /// sessions — one row too far up to be drawn whole — is drawn to the
    /// edge, its top border off screen and its name row on the window's
    /// first, where a card-sized hole used to stand. The row of air over
    /// the grid says what is above, the row kept under it what is below,
    /// and the header counts both.
    #[test]
    fn a_card_cut_by_the_top_edge_is_drawn_to_the_edge_not_dropped() {
        use crate::launcher::HEAD_H;
        use nebula_core::{TerminalId, TerminalTab, WorktreeId};
        let mut app = a_crowded_tree(9);
        select(&mut app, "api");
        app.launcher.launcher_expanded = Some(WorktreeId("w0".into()));
        for i in 1..=3 {
            app.tree.terminals.push(TerminalTab {
                id: TerminalId(format!("t{i}")),
                worktree_id: WorktreeId("w0".into()),
                name: format!("term-{i}"),
                sort_order: i,
                alive: true,
                run_command: None,
            });
        }
        app.nav.sel_session = app
            .visible_session_rows()
            .iter()
            .position(|row| matches!(row, crate::app::SessionRow::Terminal(t) if t.id.0 == "t1"))
            .expect("a row for the terminal");
        let body = Rect::new(0, 0, 100, 58);
        let lines = drawn_lines(&mut app, body);
        assert_eq!(
            app.launcher.launcher_scroll, 2,
            "two rows, the least that shows the cursor's card"
        );
        let head = &lines[1];
        assert!(head.contains("↑↓ 4 hidden"), "{head:?}");
        let air = &lines[usize::from(HEAD_H) - 1];
        assert!(air.contains("↑ 2 more above"), "{air:?}");
        let top = &lines[usize::from(HEAD_H)];
        assert!(
            top.contains("session "),
            "the cut card's name row is the window's first: {top:?}"
        );
        assert!(!top.contains('╭'), "its top border is off screen: {top:?}");
        let last = lines.last().unwrap();
        assert!(last.contains("↓ 2 more below"), "{last:?}");
        // The cursor's card is whole: its heavy bottom border on the
        // window's last row, just over the marker.
        let bottom = &lines[lines.len() - 2];
        assert!(bottom.contains('┗'), "{bottom:?}");
        assert!(lines.iter().any(|l| l.contains("term-1")), "{lines:#?}");
    }

    /// A grid with room for every card keeps the rule it has today —
    /// the marker is a thing that appears, not a thing that is always
    /// there saying zero.
    #[test]
    fn a_grid_that_fits_says_nothing_about_hiding() {
        use crate::launcher::{BAND_RULE_H, CARD_H, GAP_Y, HEAD_H};
        let mut app = a_crowded_tree(2);
        select(&mut app, "api");
        let body = Rect::new(0, 0, 60, HEAD_H + BAND_RULE_H + CARD_H * 2 + GAP_Y);
        let lines = drawn_lines(&mut app, body);
        let head = &lines[1];
        assert!(head.contains("2 sessions"), "{head:?}");
        assert!(!head.contains("hidden"), "{head:?}");
    }

    /// A terminal's card is two session cards wide and carries, under its
    /// name and what runs in it, the last lines its shell printed — the
    /// tail the daemon answered with — and the frame notes the card for
    /// the next ask. Once the shell is gone the lines stay and `exited`
    /// sits at the name row's right: the card says what it was doing
    /// when it went.
    #[test]
    fn a_terminal_card_shows_what_its_shell_last_printed() {
        use crate::app::TerminalTail;
        use nebula_core::{TerminalId, TerminalTab, WorktreeId};
        let mut app = a_tree();
        select(&mut app, "api");
        app.tree.terminals.push(TerminalTab {
            id: TerminalId("t1".into()),
            worktree_id: WorktreeId("w0".into()),
            name: "shell-1".into(),
            sort_order: 0,
            alive: true,
            run_command: None,
        });
        app.pane.terminal_tails.insert(
            TerminalId("t1".into()),
            TerminalTail {
                lines: vec!["$ npm test".into(), "ok 12 tests".into()],
                end_seq: 9,
            },
        );
        // The band open: the terminal's card is under the session's
        // rather than off the end of its one-column strip.
        app.launcher.launcher_expanded = Some(WorktreeId("w0".into()));
        let body = Rect::new(0, 0, 100, 30);
        let lines = drawn_lines(&mut app, body);
        let row = lines
            .iter()
            .position(|l| l.contains("shell-1"))
            .expect("the terminal's card");
        assert!(
            lines[row + 1].contains("shell"),
            "what runs in it: {:?}",
            lines[row + 1]
        );
        assert!(
            lines[row + 2].contains("$ npm test"),
            "{:?}",
            lines[row + 2]
        );
        assert!(
            lines[row + 3].contains("ok 12 tests"),
            "{:?}",
            lines[row + 3]
        );
        assert!(!lines[row].contains("exited"), "alive: {:?}", lines[row]);
        let widths: Vec<u16> = app
            .chrome
            .hits
            .iter()
            .filter(|(_, h)| matches!(h, HitTarget::LauncherCard(_)))
            .map(|(r, _)| r.width)
            .collect();
        assert_eq!(widths.len(), 2, "the session and the terminal: {widths:?}");
        assert_eq!(
            widths[1],
            widths[0] * 2 + crate::launcher::GAP_X,
            "the terminal's card spans two columns"
        );
        assert_eq!(
            app.pane.tail_cards,
            vec![TerminalId("t1".into())],
            "the frame notes the terminal for the grid's next ask"
        );

        app.tree.terminals[0].alive = false;
        let lines = drawn_lines(&mut app, body);
        let row = lines
            .iter()
            .position(|l| l.contains("shell-1"))
            .expect("the terminal's card");
        assert!(lines[row].contains("exited │"), "{:?}", lines[row]);
        assert!(
            lines[row + 2].contains("$ npm test"),
            "the lines stay: {:?}",
            lines[row + 2]
        );
    }

    /// A terminal card's tail is a small terminal: the shell's colours
    /// and weights per run, its defaults in the card's text colour over
    /// the card's own background (the focus tint shows through), a row
    /// cut at the card's edge with no `…`, and the cursor a reversed
    /// cell — a block past the text, the character's own colours
    /// flipped over one — only while the shell lives.
    #[test]
    fn a_terminal_cards_tail_paints_like_a_small_terminal() {
        use crate::terminal_tail::parse_tail;
        let th = App::new().chrome.theme;
        let r = Rect::new(0, 0, 12, 1);
        let paint = |data: &[u8], alive: bool| {
            let mut buf = Buffer::empty(r);
            buf.set_style(r, Style::default().bg(th.focus_tint));
            let rows = parse_tail(data, 80, 24, 4);
            draw_tail_row(&mut buf, r, rows.last().unwrap(), alive, th);
            buf
        };
        let text = |buf: &Buffer| -> String {
            (0..r.width)
                .map(|x| buf[(x, 0)].symbol().to_string())
                .collect()
        };

        let buf = paint(b"\x1b[1;32mok\x1b[0m done \x1b[33mwith a long tail", true);
        assert_eq!(text(&buf), "ok done with", "cut at the edge, no ellipsis");
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(2));
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(3, 0)].fg, th.text, "the default is the card's text");
        assert_eq!(buf[(3, 0)].bg, th.focus_tint, "the tint shows through");
        assert_eq!(buf[(8, 0)].fg, Color::Indexed(3));
        assert!(
            !buf[(11, 0)].modifier.contains(Modifier::REVERSED),
            "the cursor is past the edge"
        );

        let buf = paint(b"\x1b[31m$\x1b[0m ", true);
        assert_eq!(buf[(2, 0)].fg, th.text, "past the text: a block");
        assert!(buf[(2, 0)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(0, 0)].modifier.contains(Modifier::REVERSED));

        let buf = paint(b"\x1b[31mabc\x1b[2D", true);
        assert_eq!(
            buf[(1, 0)].fg,
            Color::Indexed(1),
            "over a character: its own colour"
        );
        assert!(buf[(1, 0)].modifier.contains(Modifier::REVERSED));

        let buf = paint(b"$ ", false);
        assert!(
            !buf[(2, 0)].modifier.contains(Modifier::REVERSED),
            "an exited shell has no cursor"
        );
    }

    /// HIDE CARD MARKS: off, a shell's card opens on `❯`, a RUN
    /// TERMINAL's on `▶` and a session card's prompt on `›`; on, none of
    /// them is drawn and each name or prompt starts in the column its mark
    /// held.
    #[test]
    fn a_session_card_shows_its_context_unless_marks_are_hidden() {
        let mut app = a_tree();
        select(&mut app, "api");
        let id = app.tree.agents[0].id.clone();
        app.launcher.session_contexts.insert(
            id,
            nebula_core::usage::ContextUsage {
                percent: Some(42.0),
                tokens: Some(84_000),
                window: Some(200_000),
                compactions: 2,
                last_compact_at: None,
                source: nebula_core::usage::UsageSource::Statusline,
                at: 0,
            },
        );
        let body = Rect::new(0, 0, 100, 40);
        let lines = drawn_lines(&mut app, body);
        assert!(
            lines.iter().any(|l| l.contains("ctx 42% \u{27f3}2")),
            "{lines:#?}"
        );
        app.launcher.hide_card_marks = true;
        let lines = drawn_lines(&mut app, body);
        assert!(!lines.iter().any(|l| l.contains("ctx 42%")));
    }

    /// WHY ITS TURN STOPPED takes the right end of the card's second row
    /// ahead of the context and the issue number, in the tone's color:
    /// the running color for a wait the session carries out itself, the
    /// needs-you color for a login it cannot, the stopped color for an
    /// error. The issue number gives up its click target with its place,
    /// and HIDE CARD MARKS leaves the note off.
    #[test]
    fn a_session_card_says_why_its_turn_stopped_ahead_of_its_context() {
        use nebula_core::stop::{StopNote, AUTHENTICATION_FAILED, RATE_LIMIT};
        let mut app = a_tree();
        select(&mut app, "api");
        let th = app.chrome.theme;
        let id = app.tree.agents[0].id.clone();
        app.launcher.session_contexts.insert(
            id.clone(),
            nebula_core::usage::ContextUsage {
                percent: Some(42.0),
                tokens: Some(84_000),
                window: Some(200_000),
                compactions: 2,
                last_compact_at: None,
                source: nebula_core::usage::UsageSource::Statusline,
                at: 0,
            },
        );
        let note = |reason: &str| StopNote {
            reason: reason.into(),
            waiting: reason == RATE_LIMIT,
            resumes: (reason == RATE_LIMIT).then(|| "3:45pm".into()),
            resets_at: None,
            detail: None,
            at: 0,
        };
        let body = Rect::new(0, 0, 100, 40);
        // The color the first drawn `text` is in.
        let fg_of = |app: &mut App, text: &str| -> Option<Color> {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(body.width, body.height))
                    .unwrap();
            terminal.draw(|f| draw(f, app, body)).unwrap();
            let buf = terminal.backend().buffer().clone();
            (0..body.height).find_map(|y| {
                let cells: Vec<&str> = (0..body.width)
                    .map(|x| buf.cell((x, y)).map_or(" ", |c| c.symbol()))
                    .collect();
                (0..cells.len())
                    .find(|&x| cells[x..].concat().starts_with(text))
                    .map(|x| buf[(x as u16, y)].fg)
            })
        };

        let lines = drawn_lines(&mut app, body);
        assert!(lines.iter().any(|l| l.contains("ctx 42%")), "{lines:#?}");

        // Waiting out a limit: the screen's words, in the running color,
        // and the context steps aside for them.
        app.launcher
            .session_stops
            .insert(id.clone(), note(RATE_LIMIT));
        let lines = drawn_lines(&mut app, body);
        assert!(
            lines.iter().any(|l| l.contains("resumes 3:45pm")),
            "{lines:#?}"
        );
        assert!(!lines.iter().any(|l| l.contains("ctx 42%")), "{lines:#?}");
        assert_eq!(fg_of(&mut app, "resumes 3:45pm"), Some(th.warn));

        // Logged out: the user has to act, the needs-you color.
        app.launcher
            .session_stops
            .insert(id.clone(), note(AUTHENTICATION_FAILED));
        let lines = drawn_lines(&mut app, body);
        assert!(lines.iter().any(|l| l.contains("logged out")), "{lines:#?}");
        assert!(!lines.iter().any(|l| l.contains("resumes")), "{lines:#?}");
        assert_eq!(fg_of(&mut app, "logged out"), Some(th.err));

        // An error nobody need act on: the stopped color.
        app.launcher
            .session_stops
            .insert(id.clone(), note("overloaded"));
        assert_eq!(fg_of(&mut app, "overloaded"), Some(th.special));

        // The marks hidden, the note goes with them; with the note
        // cleared the context is back.
        app.launcher.hide_card_marks = true;
        let lines = drawn_lines(&mut app, body);
        assert!(
            !lines.iter().any(|l| l.contains("overloaded")),
            "{lines:#?}"
        );
        app.launcher.hide_card_marks = false;
        app.launcher.session_stops.remove(&id);
        let lines = drawn_lines(&mut app, body);
        assert!(lines.iter().any(|l| l.contains("ctx 42%")), "{lines:#?}");

        // On an issue session the note covers the number, and the number's
        // click target goes with it.
        app.tree.agents[0].issue_url = Some("https://github.com/o/demo/issues/15".into());
        app.launcher.card_issue_number = true;
        let has_link = |app: &App| {
            app.chrome
                .hits
                .iter()
                .any(|(_, h)| matches!(h, HitTarget::LauncherCardIssue(_)))
        };
        app.chrome.hits.clear();
        let runs_row = |lines: &[String]| {
            let row = lines
                .iter()
                .position(|l| l.contains("s0"))
                .expect("the session's card drawn");
            lines[row + 1].clone()
        };
        let lines = drawn_lines(&mut app, body);
        assert!(runs_row(&lines).contains("#15"), "{lines:#?}");
        assert!(has_link(&app), "the number is a link while it shows");
        app.launcher
            .session_stops
            .insert(id, note(AUTHENTICATION_FAILED));
        app.chrome.hits.clear();
        let lines = drawn_lines(&mut app, body);
        assert!(runs_row(&lines).contains("logged out"), "{lines:#?}");
        assert!(!runs_row(&lines).contains("#15"), "{lines:#?}");
        assert!(!has_link(&app), "nothing to click where the note is");
    }

    /// The note needs the card to hold its words, a cell to spare, beside
    /// the harness text; a narrower card keeps what it had.
    #[test]
    fn a_card_too_narrow_for_the_stop_note_keeps_what_it_had() {
        use nebula_core::stop::{StopNote, RATE_LIMIT};
        let mut app = a_tree();
        let th = app.chrome.theme;
        let a = app.tree.agents[0].clone();
        app.launcher.session_stops.insert(
            a.id.clone(),
            StopNote {
                reason: RATE_LIMIT.into(),
                waiting: true,
                resumes: Some("3:45pm".into()),
                resets_at: None,
                detail: None,
                at: 0,
            },
        );
        // `resumes 3:45pm` is fourteen cells.
        assert_eq!(card_stop(&app, &a, 18, th), None);
        assert_eq!(
            card_stop(&app, &a, 19, th),
            Some(("resumes 3:45pm".into(), th.warn))
        );
        app.launcher.hide_card_marks = true;
        assert_eq!(card_stop(&app, &a, 40, th), None);
    }

    #[test]
    fn hide_card_marks_drops_the_mark_before_the_name_and_the_prompt() {
        use nebula_core::{TerminalId, TerminalTab, WorktreeId};
        let mut app = a_tree();
        select(&mut app, "api");
        app.tree.agents[0].recent_prompts = vec![nebula_core::PromptEntry {
            text: "fix the login redirect".into(),
            submitted_at: 0,
        }];
        for (id, name, run) in [
            ("t1", "shell-1", None),
            ("t2", "dev-srv", Some("npm run dev")),
        ] {
            app.tree.terminals.push(TerminalTab {
                id: TerminalId(id.into()),
                worktree_id: WorktreeId("w0".into()),
                name: name.into(),
                sort_order: 0,
                alive: true,
                run_command: run.map(Into::into),
            });
        }
        app.launcher.launcher_expanded = Some(WorktreeId("w0".into()));
        let body = Rect::new(0, 0, 100, 40);
        let row_of = |lines: &[String], name: &str| -> String {
            lines
                .iter()
                .find(|l| l.contains(name))
                .unwrap_or_else(|| panic!("{name}'s card: {lines:?}"))
                .clone()
        };

        let lines = drawn_lines(&mut app, body);
        assert!(row_of(&lines, "shell-1").contains("❯ shell-1"));
        assert!(row_of(&lines, "dev-srv").contains("▶ dev-srv"));
        let shown_col = row_of(&lines, "shell-1").find("❯").unwrap();
        let prompt = row_of(&lines, "fix the login redirect");
        assert!(prompt.contains("› fix the login redirect"), "{prompt:?}");
        let mark_col = prompt.find('›').unwrap();

        app.launcher.hide_card_marks = true;
        let lines = drawn_lines(&mut app, body);
        let shell = row_of(&lines, "shell-1");
        let run = row_of(&lines, "dev-srv");
        assert!(
            !shell.contains('❯') && !run.contains('▶'),
            "{shell:?} {run:?}"
        );
        assert_eq!(shell.find("shell-1"), Some(shown_col), "{shell:?}");
        let prompt = row_of(&lines, "fix the login redirect");
        assert!(!prompt.contains('›'), "{prompt:?}");
        assert_eq!(
            prompt.find("fix the login redirect"),
            Some(mark_col),
            "{prompt:?}"
        );
    }

    /// A session card's second row says what the session runs on — the
    /// harness, the model and the reasoning effort it was launched with,
    /// `claude opus high` — and a session on the CLI's own default effort
    /// ends at the model.
    #[test]
    fn a_session_card_names_its_effort_beside_the_model() {
        let mut app = a_tree();
        select(&mut app, "api");
        app.tree.agents[0].model = Some("opus".into());
        app.tree.agents[0].effort = Some("high".into());
        let body = Rect::new(0, 0, 100, 30);
        let lines = drawn_lines(&mut app, body);
        let row = lines
            .iter()
            .position(|l| l.contains("s0"))
            .expect("the session's card");
        assert!(
            lines[row + 1].contains("claude opus high"),
            "what it runs on: {:?}",
            lines[row + 1]
        );

        app.tree.agents[0].effort = None;
        let lines = drawn_lines(&mut app, body);
        let row = lines
            .iter()
            .position(|l| l.contains("s0"))
            .expect("the session's card");
        assert!(
            lines[row + 1].contains("claude opus") && !lines[row + 1].contains("high"),
            "the CLI's default effort is unnamed: {:?}",
            lines[row + 1]
        );
    }

    /// Every card carries its session's last prompt under the name and
    /// harness — there is no setting to leave it off any more.
    #[test]
    fn every_card_shows_its_last_prompt() {
        let mut app = a_tree();
        select(&mut app, "api");
        app.tree.agents[0].recent_prompts = vec![nebula_core::PromptEntry {
            text: "fix the login redirect".into(),
            submitted_at: 0,
        }];
        let body = Rect::new(0, 0, 100, 30);
        let shows = |lines: &[String]| lines.iter().any(|l| l.contains("fix the login redirect"));

        let lines = drawn_lines(&mut app, body);
        assert!(shows(&lines), "{lines:#?}");
        let row = lines
            .iter()
            .position(|l| l.contains("s0"))
            .expect("the session's card");
        assert!(lines[row + 1].contains("claude"), "{:?}", lines[row + 1]);
    }

    /// The `card_issue_number` setting puts an ISSUE SESSION's `#15` at
    /// the right end of the row saying what it runs on — only with the
    /// setting on, only on a card started from an issue — and registers
    /// it as a click target ahead of the card.
    #[test]
    fn card_issue_number_shows_the_issue_on_the_runs_on_row() {
        let mut app = a_tree();
        select(&mut app, "api");
        app.tree.agents[0].issue_url = Some("https://github.com/o/r/issues/15".into());
        let body = Rect::new(0, 0, 100, 30);
        let runs_row = |lines: &[String]| {
            let row = lines
                .iter()
                .position(|l| l.contains("s0"))
                .expect("the session's card drawn");
            lines[row + 1].clone()
        };

        let lines = drawn_lines(&mut app, body);
        assert!(!runs_row(&lines).contains("#15"), "off by default");
        assert!(!app
            .chrome
            .hits
            .iter()
            .any(|(_, h)| matches!(h, HitTarget::LauncherCardIssue(_))));

        app.launcher.card_issue_number = true;
        app.chrome.hits.clear();
        let lines = drawn_lines(&mut app, body);
        let row = runs_row(&lines);
        assert!(row.contains("claude"), "{row:?}");
        assert!(row.contains("#15"), "{row:?}");
        assert!(
            row.find("claude") < row.find("#15"),
            "after what it runs on: {row:?}"
        );
        let id = app.tree.agents[0].id.clone();
        let chip = app
            .hit_rect(&HitTarget::LauncherCardIssue(id.clone()))
            .expect("the number is a click target");
        assert_eq!(
            app.hit_at(chip.x, chip.y),
            Some(HitTarget::LauncherCardIssue(id)),
            "registered ahead of the card, so it wins"
        );
        let drawn: String = lines[usize::from(chip.y)]
            .chars()
            .skip(usize::from(chip.x))
            .take(usize::from(chip.width))
            .collect();
        assert_eq!(drawn, "#15", "the target is the text");
    }

    /// The runs-on line is the harness, then the model, then the effort,
    /// each only while the row carries it; a cloud row names the sandbox
    /// in place of harness and model and keeps the effort, as the pane's
    /// breadcrumb does.
    #[test]
    fn the_runs_on_line_is_harness_model_effort() {
        let mut a = a_tree().tree.agents[0].clone();
        assert_eq!(runs_on_line(&a, &mut None), "claude");
        a.effort = Some("high".into());
        assert_eq!(runs_on_line(&a, &mut None), "claude high");
        a.model = Some("opus".into());
        assert_eq!(runs_on_line(&a, &mut None), "claude opus high");
        a.cloud_session_id = Some("c1".into());
        assert_eq!(runs_on_line(&a, &mut None), "cloud high");
    }

    /// Every row of `body` as drawn with the view on it.
    fn drawn_lines(app: &mut App, body: Rect) -> Vec<String> {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(body.width, body.height))
                .unwrap();
        terminal.draw(|f| draw(f, app, body)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..body.height)
            .map(|y| {
                (0..body.width)
                    .filter_map(|x| buf.cell((x, y)))
                    .map(|c| c.symbol().to_string())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn columns_layout_draws_the_three_existing_row_lists() {
        let mut app = a_tree();
        app.launcher.launcher_columns = true;
        app.launcher.columns_recent_prompts = true;
        app.tree.agents[0].recent_prompts = vec![nebula_core::PromptEntry {
            text: "fix the columns layout".into(),
            submitted_at: 0,
        }];
        app.nav.focus = Focus::Projects;

        let lines = drawn_lines(&mut app, Rect::new(0, 0, 96, 12)).join("\n");
        assert!(lines.contains("PROJECTS"), "{lines}");
        assert!(lines.contains("WORKTREES"), "{lines}");
        assert!(lines.contains("SESSIONS"), "{lines}");
        assert!(lines.contains("api"), "{lines}");
        assert!(lines.contains("feat"), "{lines}");
        assert!(lines.contains("s0"), "{lines}");
        assert!(lines.contains("fix the columns"), "{lines}");
        assert!(app.hit_rect(&HitTarget::ProjectRow(0)).is_some());
        assert!(app.hit_rect(&HitTarget::WorktreeRow(0)).is_some());
        assert!(app.hit_rect(&HitTarget::SessionRow(0)).is_some());
    }

    /// `a_tree` with `count` checkouts in `api` instead of one, a session
    /// in each, so the grid can be given more bands than the body has
    /// room for.
    fn a_tree_with_worktrees(count: usize) -> App {
        use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, Worktree, WorktreeId};
        let mut app = a_tree();
        let one = app.tree.agents[0].clone();
        app.tree.worktrees = (0..count)
            .map(|i| Worktree {
                id: WorktreeId(format!("wt{i}")),
                project_id: nebula_core::ProjectId("p0".into()),
                path: format!("/tmp/wt{i}").into(),
                branch: format!("feat-{i}"),
                is_main: false,
                sort_order: i as i64,
            })
            .collect();
        app.tree.agents = (0..count)
            .map(|i| Agent {
                parent: None,
                id: AgentId(format!("api{i}")),
                worktree_id: WorktreeId(format!("wt{i}")),
                name: format!("session {i}"),
                status: AgentStatus::Finished,
                kind: AgentKind::Claude,
                ..one.clone()
            })
            .collect();
        app.launcher.launcher_expanded = None;
        app
    }

    /// Put the cursor on the band of checkout `id`, at the band level.
    fn aim_at_band(app: &mut App, id: &str) {
        app.nav.sel_worktree = app
            .worktree_row_of(&nebula_core::WorktreeId(id.into()))
            .expect("a row for the checkout");
        app.launcher.launcher_expanded = None;
    }

    /// The air under the last whole band says how many more are down
    /// there — in that air, under the bands, not over them — and once
    /// the walk down has brought the last one on screen the cue is the
    /// header's row of air pointing back up at the ones scrolled off.
    #[test]
    fn the_air_under_the_cards_says_how_many_more_are_below() {
        use crate::launcher::{BAND_H, GAP_Y, HEAD_H};
        let mut app = a_tree_with_worktrees(9);
        select(&mut app, "api");
        aim_at_band(&mut app, "wt0");
        // Room for two bands and four rows of air under them.
        let bands_end = HEAD_H + BAND_H * 2 + GAP_Y;
        let body = Rect::new(0, 0, 60, bands_end + 4);
        let lines = drawn_lines(&mut app, body);
        let cue: Vec<usize> = (0..lines.len())
            .filter(|&y| lines[y].contains("more worktree"))
            .collect();
        assert_eq!(cue.len(), 1, "{lines:#?}");
        // Seven bands are past the two whole ones — the third's rule fits
        // in the air, its cards cut, and a band drawn cut still counts.
        assert!(
            lines[cue[0]].contains("↓ 7 more worktrees below"),
            "{:?}",
            lines[cue[0]]
        );
        assert!(cue[0] >= bands_end as usize, "under the bands: {lines:#?}");

        // Walked to the last band, nothing is left below to point at: the
        // seven scrolled off the top are counted on the row of air under
        // the tabs instead.
        aim_at_band(&mut app, "wt8");
        let lines = drawn_lines(&mut app, body);
        assert!(
            lines
                .iter()
                .all(|line| !line.contains("more worktrees below")),
            "{lines:#?}"
        );
        assert!(
            lines[usize::from(HEAD_H) - 1].contains("↑ 7 more worktrees above"),
            "{lines:#?}"
        );
    }

    /// A grid with every band on screen leaves its air empty, however
    /// much of it there is.
    #[test]
    fn a_grid_that_fits_leaves_its_air_empty() {
        use crate::launcher::{BAND_H, HEAD_H};
        let mut app = a_tree_with_worktrees(2);
        select(&mut app, "api");
        aim_at_band(&mut app, "wt0");
        let body = Rect::new(0, 0, 60, HEAD_H + BAND_H * 2 + 6);
        let lines = drawn_lines(&mut app, body);
        assert!(
            lines.iter().all(|line| !line.contains("more worktree")),
            "{lines:#?}"
        );
    }

    /// Put the PROJECTS cursor on the named project. The cursor is a row
    /// index and a turn starting anywhere reorders the rows, so a test
    /// that starts one has to say again which project the grid is on.
    fn select(app: &mut App, name: &str) {
        app.nav.sel_project = app
            .project_rows()
            .iter()
            .position(|i| app.tree.projects[*i].name == name)
            .expect("a row for the project");
    }

    /// The HIDDEN MARKER: a grid that holds every card says nothing, and
    /// one that does not says how many it left off and which way they
    /// went — so a PANE dragged up over the cards reads as the pane
    /// taking their room rather than as sessions going missing.
    #[test]
    fn the_header_says_how_many_cards_it_could_not_fit() {
        let app = App::new();
        let th = app.chrome.theme;
        let mark = |hidden| {
            let text = row_text(&count_spans(&app, 9, hidden, 80, th));
            text.strip_suffix(TOGGLE_ROW)
                .unwrap_or_else(|| panic!("the toggle ends the row: {text:?}"))
                .to_string()
        };

        assert_eq!(mark(Hidden::default()), "9 sessions", "nothing is missing");
        assert_eq!(
            mark(Hidden { above: 0, below: 5 }),
            "9 sessions  ↓ 5 hidden",
            "under the fold: step down to them"
        );
        assert_eq!(
            mark(Hidden { above: 4, below: 0 }),
            "9 sessions  ↑ 4 hidden"
        );
        assert_eq!(
            mark(Hidden { above: 2, below: 3 }),
            "9 sessions  ↑↓ 5 hidden",
            "both ways at once, counted together"
        );
    }

    /// The counts get what the tabs leave them and give way whole, least
    /// needed first — the count of the cards, then the PR & ISSUE COUNTS
    /// — with the HIDDEN MARKER last, and never run past their room.
    #[test]
    fn the_counts_give_way_a_piece_at_a_time() {
        let mut app = a_tree();
        select(&mut app, "api");
        let id = app.selected_project().expect("api").id.clone();
        app.github.issues.insert(
            id,
            crate::issues::IssueList {
                list: vec![crate::issues::Issue {
                    number: 7,
                    url: "https://github.com/o/r/issues/7".into(),
                    title: "Tabs vanish on a narrow window".into(),
                    author: "webdevcody".into(),
                    created_at: "2026-09-10T12:00:00Z".into(),
                    updated_at: "2026-09-11T12:00:00Z".into(),
                    labels: Vec::new(),
                    body: String::new(),
                }],
                at: std::time::Instant::now(),
            },
        );
        let th = app.chrome.theme;
        let hidden = Hidden { above: 0, below: 5 };
        let mark_w = "  ↓ 5 hidden".chars().count();
        let toggle_w = "   Overview │ Board ".chars().count();
        for width in 0..=120usize {
            let spans = count_spans(&app, 9, hidden, width, th);
            let text = row_text(&spans);
            let used: usize = spans.iter().map(|s| s.width()).sum();
            assert!(used <= width, "{width}: {text:?} overruns");
            let toggle = text.contains("Overview │ Board");
            let words = text.contains("9 sessions");
            let badge = text.contains("1 issue");
            let mark = text.contains("5 hidden");
            assert_eq!(toggle, width >= toggle_w, "{width}: {text:?}");
            assert_eq!(mark, width >= toggle_w + mark_w, "{width}: {text:?}");
            assert!(!badge || mark, "{width}: the marker went first: {text:?}");
            assert!(!words || badge, "{width}: the counts went first: {text:?}");
            let tools = text.contains('⚙') && text.contains(" ? ");
            assert!(!tools || words, "{width}: the tools went first: {text:?}");
        }
        assert_eq!(
            row_text(&count_spans(&app, 9, hidden, 120, th)),
            format!("9 sessions  1 issue  ↓ 5 hidden{TOGGLE_ROW}"),
            "a column of air more between the count and the badge"
        );
    }

    /// The VIEW TOGGLE as the row ends in it, its key and the HEADER
    /// TOOLS after it included.
    const TOGGLE_ROW: &str = "   Overview │ Board  ⇧K  ⚙  ? ";

    /// A session on a profile names it, the default one too: the label
    /// never changes because the default moved. No profile, no `@`.
    #[test]
    fn the_harness_line_always_names_the_profile() {
        let mut app = a_tree();
        let a = &mut app.tree.agents[0];
        a.model = Some("opus".into());
        assert_eq!(harness_line(a, &mut None), "claude opus");
        a.profile = Some("main".into());
        assert_eq!(harness_line(a, &mut None), "claude@main opus");
        a.effort = Some("high".into());
        assert_eq!(runs_on_line(a, &mut None), "claude@main opus high");
    }

    #[test]
    fn the_status_word_is_the_desktops() {
        use nebula_core::AgentStatus as S;
        assert_eq!(status_word(S::NeedsFeedback, false), "needs you");
        assert_eq!(status_word(S::Running, true), "running");
        assert_eq!(status_word(S::Finished, true), "unread");
        assert_eq!(status_word(S::Finished, false), "done");
        assert_eq!(status_word(S::Terminated, false), "stopped");
        assert_eq!(status_word(S::Fresh, false), "idle");
    }

    /// The pane header's facts give way whole, the least telling first,
    /// and never overrun; the last prompt only takes room the rest left.
    #[test]
    fn the_pane_facts_give_way_a_piece_at_a_time() {
        let th = App::new().chrome.theme;
        let facts = PaneFacts {
            status: ("running", th.warn),
            stop: None,
            runs_on: "claude@max2 opus".into(),
            place: ("↳ ", "api / feat".into(), th.worktree),
            ctx: Some(("ctx 42%".into(), th.muted)),
            pr: Some(("#12 ready".into(), th.accent)),
            age: Some("3m".into()),
            prompt: Some("fix the login redirect loop on expired sessions".into()),
        };
        let text = |room| -> String {
            let spans = fact_spans(&facts, room, th);
            let used: usize = spans.iter().map(Span::width).sum();
            assert!(used <= room, "{room}: overruns");
            spans.iter().map(|s| s.content.as_ref()).collect()
        };
        assert_eq!(
            text(200),
            "  running  claude@max2 opus  ↳ api / feat  ctx 42%  #12 ready  3m  › fix the login redirect loop on expired sessions"
        );
        assert_eq!(
            text(64),
            "  running  claude@max2 opus  ↳ api / feat  ctx 42%  #12 ready"
        );
        assert_eq!(text(30), "  running  claude@max2 opus");
        assert_eq!(text(8), "");
        for room in 0..200 {
            let t = text(room);
            let has = |s: &str| t.contains(s);
            assert!(!has("claude") || has("running"), "{room}: {t}");
            assert!(!has("api / feat") || has("claude"), "{room}: {t}");
            assert!(!has("ctx") || has("api / feat"), "{room}: {t}");
            assert!(!has("#12") || has("ctx"), "{room}: {t}");
            assert!(!has("› ") || has("3m"), "{room}: {t}");
        }
    }

    /// The pane header's STOP NOTE comes right after the status word it
    /// explains and gives way after the facts behind it, before the
    /// status itself; a session with a note carries it into its facts in
    /// the tone's color.
    #[test]
    fn the_pane_facts_put_a_stop_note_right_after_the_status() {
        use nebula_core::stop::{StopNote, RATE_LIMIT};
        let mut app = a_tree();
        let th = app.chrome.theme;
        let id = app.tree.agents[0].id.clone();
        let row = crate::launcher::row(&app, &id).expect("a row for the session");
        assert_eq!(pane_facts(&app, &row, false, 0).stop, None);
        app.launcher.session_stops.insert(
            id,
            StopNote {
                reason: RATE_LIMIT.into(),
                waiting: true,
                resumes: Some("3:45pm".into()),
                resets_at: None,
                detail: None,
                at: 0,
            },
        );
        assert_eq!(
            pane_facts(&app, &row, false, 0).stop,
            Some(("resumes 3:45pm".into(), th.warn))
        );

        let facts = PaneFacts {
            status: ("running", th.warn),
            stop: Some(("resumes 3:45pm".into(), th.warn)),
            runs_on: "claude@max2 opus".into(),
            place: ("↳ ", "api / feat".into(), th.worktree),
            ctx: None,
            pr: None,
            age: Some("3m".into()),
            prompt: None,
        };
        let text = |room| -> String {
            let spans = fact_spans(&facts, room, th);
            let used: usize = spans.iter().map(Span::width).sum();
            assert!(used <= room, "{room}: overruns");
            spans.iter().map(|s| s.content.as_ref()).collect()
        };
        assert_eq!(
            text(200),
            "  running  resumes 3:45pm  claude@max2 opus  ↳ api / feat  3m"
        );
        assert_eq!(
            text(42),
            "  running  resumes 3:45pm",
            "what it runs on is next, and whole or not at all"
        );
        assert_eq!(text(25), "  running  resumes 3:45pm");
        assert_eq!(text(24), "  running", "the note goes before the status");
        assert_eq!(text(8), "");
        for room in 0..200 {
            let t = text(room);
            let has = |s: &str| t.contains(s);
            assert!(!has("resumes") || has("running"), "{room}: {t}");
            assert!(!has("claude") || has("resumes"), "{room}: {t}");
            assert!(!has("api / feat") || has("claude"), "{room}: {t}");
            assert!(!has("3m") || has("api / feat"), "{room}: {t}");
        }
    }

    /// The VIEW TOGGLE says which home is on screen and is the button to
    /// the other; on the board the scope sits beside the count, a button
    /// too.
    #[test]
    fn the_header_toggles_overview_and_board_and_names_the_boards_scope() {
        let mut app = App::new();
        let th = app.chrome.theme;
        let count = HeadCount {
            sessions: 2,
            terminals: 0,
        };
        let hits = |app: &App| -> Vec<HitTarget> {
            head_count(app, count, Hidden::default(), 120, th)
                .into_iter()
                .filter_map(|(_, hit)| hit)
                .collect()
        };
        assert_eq!(
            hits(&app),
            [
                HitTarget::LauncherHome(false),
                HitTarget::LauncherHome(true),
                HitTarget::Button(crate::keymap::Action::Settings),
                HitTarget::Button(crate::keymap::Action::Help),
            ]
        );
        let lit = |app: &App, label: &str| {
            head_count(app, count, Hidden::default(), 120, th)
                .into_iter()
                .find(|(s, _)| s.content.trim() == label)
                .is_some_and(|(s, _)| s.style.bg == Some(th.sel_bg))
        };
        assert!(lit(&app, "Overview") && !lit(&app, "Board"));

        app.launcher.launcher_kanban = true;
        assert!(lit(&app, "Board") && !lit(&app, "Overview"));
        // On the board, the SCOPE CHIP: `all` with nothing narrowed, a
        // button back to everything.
        assert!(hits(&app).contains(&HitTarget::ScopeChip(0)));
        let text = row_text(&count_spans(&app, 2, Hidden::default(), 120, th));
        assert!(text.starts_with("2 sessions · all"), "{text:?}");
        assert!(
            !hits(&app).contains(&HitTarget::ScopeClear),
            "nothing to clear"
        );
    }

    /// [`head_count`]'s spans without the buttons they carry.
    fn count_spans(
        app: &App,
        count: usize,
        hidden: Hidden,
        width: usize,
        th: Theme,
    ) -> Vec<Span<'static>> {
        let count = HeadCount {
            sessions: count,
            terminals: 0,
        };
        head_count(app, count, hidden, width, th)
            .into_iter()
            .map(|(span, _)| span)
            .collect()
    }

    /// The row as one string, the way it lands on screen.
    fn row_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// `a_tree` on `api`, with both projects open: `web` opened last, so
    /// its tab leads.
    fn a_tabbed_tree() -> App {
        let mut app = a_tree();
        select(&mut app, "api");
        app.launcher.launcher_tabs = vec![ProjectId("p1".into()), ProjectId("p0".into())];
        app
    }

    use nebula_core::ProjectId;

    /// The header's buttons, in the order they were laid down.
    fn head_hits(app: &App) -> Vec<HitTarget> {
        app.chrome.hits.iter().map(|(_, h)| h.clone()).collect()
    }

    /// The header is a `+` and then the open projects as tabs, the newest
    /// opened next to it — where the `+` puts the next one — each with its
    /// own `×`: no `nebula`, no trail. The tab the grid is on is lit: a raised
    /// chip with its name in the accent, where the rest sit flat.
    #[test]
    fn the_header_is_the_open_projects_as_tabs() {
        let r = Rect::new(0, 0, 80, 1);
        let mut app = a_tabbed_tree();
        let th = app.chrome.theme;
        app.chrome.hits.clear();
        let spans = head_tabs(&mut app, r);
        let text = row_text(&spans);
        assert_eq!(text, " +   web ×   api × ");
        let (web, api) = (ProjectId("p1".into()), ProjectId("p0".into()));
        assert_eq!(
            head_hits(&app),
            vec![
                HitTarget::LauncherTabAdd,
                HitTarget::LauncherTab(web.clone()),
                HitTarget::LauncherTabClose(web),
                HitTarget::LauncherTab(api.clone()),
                HitTarget::LauncherTabClose(api.clone()),
            ]
        );
        // Each tab's rect is its own name and never the `×` beside it.
        let (rect, _) = app
            .chrome
            .hits
            .iter()
            .find(|(_, h)| *h == HitTarget::LauncherTab(api.clone()))
            .expect("api's tab");
        let cells: Vec<char> = text.chars().collect();
        let tab: String = cells[rect.x as usize..(rect.x + rect.width) as usize]
            .iter()
            .collect();
        assert_eq!(tab, " api", "the name, and never the × beside it");

        let lit = spans.iter().find(|s| s.content == "api").expect("api");
        assert_eq!(lit.style.fg, Some(th.accent));
        assert_eq!(lit.style.bg, Some(th.sel_bg), "the lit tab is a chip");
        let flat = spans.iter().find(|s| s.content == "web").expect("web");
        assert_eq!(flat.style.fg, Some(th.muted));
        assert_eq!(flat.style.bg, None);

        // With nothing open, the `+` says what it does.
        app.launcher.launcher_tabs.clear();
        app.chrome.hits.clear();
        let spans = head_tabs(&mut app, r);
        assert_eq!(row_text(&spans), " + open a project ");
        assert_eq!(head_hits(&app), vec![HitTarget::LauncherTabAdd]);
    }

    /// A tab's STATUS DOTS: a dot and a count per state its project's
    /// sessions are in, right of the name, in the one order — needs-you
    /// red, done blue, running yellow — with no word of its own. A quiet
    /// project is its bare name.
    #[test]
    fn a_tab_carries_its_projects_status_dots() {
        use nebula_core::{AgentId, AgentStatus};
        let r = Rect::new(0, 0, 80, 1);
        let mut app = a_tabbed_tree();
        let th = app.chrome.theme;
        // api: one asking, one mid-turn; web: one finished unread.
        let mut asking = app.tree.agents[0].clone();
        asking.id = AgentId("a9".into());
        asking.status = AgentStatus::NeedsFeedback;
        app.tree.agents.push(asking);
        app.tree.agents[0].status = AgentStatus::Running;
        app.tree.agents[1].unseen = true;
        select(&mut app, "api");
        app.chrome.hits.clear();
        let spans = head_tabs(&mut app, r);
        assert_eq!(row_text(&spans), " +   web ●1 ×   api ●1 ●1 × ");
        let dots: Vec<(String, Option<Color>)> = spans
            .iter()
            .filter(|s| s.content.contains('●'))
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect();
        assert_eq!(
            dots,
            vec![
                (" ●1".to_string(), Some(th.done)),
                (" ●1".to_string(), Some(th.err)),
                (" ●1".to_string(), Some(th.warn)),
            ]
        );
    }

    /// A lone tab carries its `×` like any other: closing it goes back
    /// to the splash (`event_loop::launcher::close_tab`).
    #[test]
    fn a_lone_tab_has_a_cross() {
        let r = Rect::new(0, 0, 80, 1);
        let mut app = a_tabbed_tree();
        app.launcher.launcher_tabs = vec![ProjectId("p0".into())];
        let spans = head_tabs(&mut app, r);
        assert_eq!(row_text(&spans), " +   api × ");
        assert!(head_hits(&app)
            .iter()
            .any(|h| matches!(h, HitTarget::LauncherTabClose(_))));

        let mut app = a_tabbed_tree();
        app.chrome.hits.clear();
        let spans = head_tabs(&mut app, r);
        assert_eq!(row_text(&spans), " +   web ×   api × ");
    }

    /// The tabs sweep in place: whatever the work under them is doing, the
    /// names spell the same names in the same columns — a running project's
    /// name is recolored a cell at a time ([`tab_ramp`]), never moved.
    #[test]
    fn the_tabs_sweep_in_place_whatever_is_running() {
        use nebula_core::AgentStatus;
        let r = Rect::new(0, 0, 80, 1);
        let mut app = a_tabbed_tree();
        app.chrome.hits.clear();
        head_tabs(&mut app, r);
        let quiet = std::mem::take(&mut app.chrome.hits);
        for a in &mut app.tree.agents {
            a.status = AgentStatus::Running;
        }
        select(&mut app, "api");
        let busy = head_tabs(&mut app, r);
        assert!(row_text(&busy).contains(" web"), "{:?}", row_text(&busy));
        assert!(
            !busy.iter().any(|s| s.content == "web"),
            "swept a cell at a time"
        );
        let busy_hits = std::mem::take(&mut app.chrome.hits);
        let first_tab = |hits: &[(Rect, HitTarget)]| {
            hits.iter()
                .find(|(_, h)| matches!(h, HitTarget::LauncherTab(_)))
                .map(|(r, h)| (r.x, h.clone()))
        };
        assert_eq!(
            first_tab(&quiet),
            first_tab(&busy_hits),
            "the first tab starts where it did"
        );
    }

    /// Red outranks everything, yellow outranks an unread finish, and blue
    /// is only a project with nothing live left; a quiet one holds still.
    #[test]
    fn a_tabs_ramp_is_the_loudest_state_under_it() {
        let th = Theme::default();
        let tally = |needs_you, running, done| Tally {
            needs_you,
            done,
            running,
        };
        assert_eq!(tab_ramp(tally(1, 3, 2), th), Some(th.err_sweep));
        assert_eq!(tab_ramp(tally(0, 1, 2), th), Some(th.warn_sweep));
        assert_eq!(tab_ramp(tally(0, 0, 2), th), Some(th.done_sweep));
        assert_eq!(tab_ramp(tally(0, 0, 0), th), None);
    }

    /// The pointer marks the button it rests on: a tab's name underlines,
    /// its `×` turns red, the `+` underlines — and nothing else does.
    #[test]
    fn the_pointer_marks_the_tab_it_rests_on() {
        let r = Rect::new(0, 0, 80, 1);
        let mut app = a_tabbed_tree();
        let th = app.chrome.theme;
        let web = ProjectId("p1".into());
        let underlined = |spans: &[Span<'static>]| -> Vec<String> {
            spans
                .iter()
                .filter(|s| s.style.add_modifier.contains(Modifier::UNDERLINED))
                .map(|s| s.content.to_string())
                .collect()
        };
        assert!(underlined(&head_tabs(&mut app, r)).is_empty());

        app.launcher.hover_crumb = Some(HitTarget::LauncherTab(web.clone()));
        assert_eq!(underlined(&head_tabs(&mut app, r)), ["web"]);

        app.launcher.hover_crumb = Some(HitTarget::LauncherTabAdd);
        assert_eq!(underlined(&head_tabs(&mut app, r)), ["+"]);

        app.launcher.hover_crumb = Some(HitTarget::LauncherTabClose(web));
        let spans = head_tabs(&mut app, r);
        assert!(underlined(&spans).is_empty());
        let crosses: Vec<Option<Color>> = spans
            .iter()
            .filter(|s| s.content.contains('×'))
            .map(|s| s.style.fg)
            .collect();
        assert_eq!(crosses, [Some(th.err), Some(th.muted)], "web's is red");
    }

    /// More tabs than the row holds: the ones it has no room for go into
    /// the MORE CHIP, which counts them, rather than being drawn half;
    /// every tab is either drawn or in the chip, the lit tab is always
    /// drawn — whole wherever there is room — the `+` is never pushed
    /// off, and nothing overruns the row at any width.
    #[test]
    fn tabs_that_do_not_fit_go_into_the_more_chip() {
        use nebula_core::Project;
        let mut app = a_tree();
        for i in 2..9 {
            app.tree.projects.push(Project {
                id: ProjectId(format!("p{i}")),
                name: format!("project-number-{i}"),
                repo_path: format!("/tmp/p{i}").into(),
                sort_order: 0,
            });
        }
        // Every project open, `api` — the oldest opened — last.
        app.launcher.launcher_tabs = (0..9).rev().map(|i| ProjectId(format!("p{i}"))).collect();
        select(&mut app, "api");
        for width in 5..=160u16 {
            let r = Rect::new(0, 0, width, 1);
            app.chrome.hits.clear();
            let spans = head_tabs(&mut app, r);
            let text = row_text(&spans);
            let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(used <= width as usize, "{width}: {text:?} overruns");
            assert!(text.contains('+'), "{width}: the + went: {text:?}");
            let drawn = head_hits(&app)
                .iter()
                .filter(|h| matches!(h, HitTarget::LauncherTab(_)))
                .count();
            let more = app.launcher.launcher_tabs_more.len();
            if drawn + more > 0 {
                assert_eq!(drawn + more, 9, "{width}: a tab went missing: {text:?}");
            }
            if more > 0 {
                assert!(
                    text.contains(&format!("{more} more ▾")),
                    "{width}: {text:?}"
                );
                assert!(head_hits(&app).contains(&HitTarget::LauncherTabMore));
            }
            if width >= 24 {
                assert!(
                    text.contains(" api × "),
                    "{width}: the lit tab went: {text:?}"
                );
            }
        }
        // Wide enough for all of them, no chip.
        let r = Rect::new(0, 0, 400, 1);
        let text = row_text(&head_tabs(&mut app, r));
        assert!(!text.contains("more"), "{text:?}");
        assert!(app.launcher.launcher_tabs_more.is_empty());
    }

    /// The tabs give way in steps, each giving up less than the next: the
    /// `×` off every tab but the lit one, then tabs from the right into the
    /// MORE CHIP, then the lit tab's name, down to [`TAB_NAME_MIN`]; then
    /// the chip alone, holding every tab, and at last nothing. The lit tab
    /// keeps its place among the tabs kept.
    #[test]
    fn the_tabs_give_way_in_steps() {
        let size = |name: usize| TabSize {
            label: 1 + name,
            cross: 3,
            name,
        };
        // Four tabs, the third lit; a chip is ten columns, whatever it holds.
        let sizes = [size(5), size(5), size(10), size(5)];
        let fit = |budget| fit_tabs(&sizes, Some(2), budget, |_| 10);
        let tabs = |shown: &[usize], crosses, lit_name, more: &[usize]| TabFit {
            shown: shown.to_vec(),
            crosses,
            lit_name,
            more: more.to_vec(),
        };
        // 29 of labels, 12 of crosses, 3 of air.
        assert_eq!(fit(44), tabs(&[0, 1, 2, 3], true, 10, &[]));
        assert_eq!(fit(43), tabs(&[0, 1, 2, 3], false, 10, &[]));
        assert_eq!(fit(35), tabs(&[0, 1, 2, 3], false, 10, &[]));
        assert_eq!(fit(34), tabs(&[0, 2], false, 10, &[1, 3]));
        assert_eq!(fit(25), tabs(&[2], false, 10, &[0, 1, 3]));
        assert_eq!(fit(24), tabs(&[2], false, 9, &[0, 1, 3]));
        assert_eq!(fit(18), tabs(&[2], false, 3, &[0, 1, 3]));
        assert_eq!(fit(17), tabs(&[], false, 10, &[0, 1, 2, 3]));
        assert_eq!(fit(9), tabs(&[], false, 10, &[]));
        // With no tab lit, the first ones are the ones kept.
        let unlit = fit_tabs(&sizes, None, 31, |_| 10);
        assert_eq!(unlit, tabs(&[0, 1], false, 0, &[2, 3]));
    }

    /// The MORE CHIP carries the STATUS DOTS of the tabs it stands in for,
    /// so a project the row had no room for still says it wants you — and
    /// the header remembers which projects those are, for the chip's list.
    #[test]
    fn the_more_chip_carries_the_dots_of_the_tabs_it_holds() {
        use nebula_core::AgentStatus;
        let mut app = a_tabbed_tree();
        let th = app.chrome.theme;
        let web = ProjectId("p1".into());
        for p in &mut app.tree.projects {
            if p.id == web {
                p.name = "website-frontend".into();
            }
        }
        // web: one waiting on you.
        app.tree.agents[1].status = AgentStatus::NeedsFeedback;
        select(&mut app, "api");
        app.chrome.hits.clear();
        let spans = head_tabs(&mut app, Rect::new(0, 0, 30, 1));
        let text = row_text(&spans);
        assert!(!text.contains("website"), "{text:?}");
        assert!(text.contains(" api × "), "the lit tab, whole: {text:?}");
        assert!(text.contains("1 more ●1 ▾"), "{text:?}");
        let dot = spans.iter().find(|s| s.content == " ●1").expect("a dot");
        assert_eq!(dot.style.fg, Some(th.err), "web's red, on the chip");
        assert_eq!(app.launcher.launcher_tabs_more, vec![web]);
        assert!(head_hits(&app).contains(&HitTarget::LauncherTabMore));
    }

    /// Squeezed, the tabs that are not lit give up their `×` before any
    /// tab gives way: the lit one keeps its own.
    #[test]
    fn a_crowded_row_keeps_only_the_lit_tabs_cross() {
        let mut app = a_tabbed_tree();
        let web = ProjectId("p1".into());
        for p in &mut app.tree.projects {
            if p.id == web {
                p.name = "website-frontend".into();
            }
        }
        app.chrome.hits.clear();
        let text = row_text(&head_tabs(&mut app, Rect::new(0, 0, 30, 1)));
        assert_eq!(text, " +   website-frontend  api × ");
        assert!(!head_hits(&app).contains(&HitTarget::LauncherTabClose(web)));
        assert!(app.launcher.launcher_tabs_more.is_empty());
    }

    /// The prompt header keeps the toggle whatever else it has to drop,
    /// never overruns its row, and hands back the columns a click on the
    /// toggle lands in. Where the launch lands is the details row's to
    /// say: the header names no project and no branch.
    #[test]
    fn the_prompt_header_never_drops_the_toggle() {
        let th = Theme::default();
        let launch = a_launch();
        let label = "what should the agent do?";
        for width in 40..=120u16 {
            let TargetLine { line, toggle } = target_line(&launch, label, width, th);
            let text = text_of(&line);
            assert!(line.width() <= width as usize, "{width}: {text:?}");
            assert!(text.contains("new worktree"), "{width}: {text:?}");
            assert!(
                !text.contains(" / ") && !text.contains("^T"),
                "{width}: the header carries no checkout crumb: {text:?}"
            );
            let (x, w) = toggle.expect("a worktree launch has a toggle to click");
            assert_eq!(usize::from(x + w), line.width(), "{width}: {text:?}");
        }
    }

    /// The branch in the details row is the WORKTREE PICKER's button: the
    /// columns handed back are exactly the branch and its `^T`, inside the
    /// `worktree` field's own, however the row had to shrink — and none
    /// once the field is dropped for room. A PR SESSION's branch is no
    /// button at all, and wears no `^T`: its checkout is the DAEMON's to
    /// pick.
    #[test]
    fn the_details_row_hands_back_the_branch_button() {
        let th = Theme::default();
        let app = App::new();
        let mut seen = 0;
        for width in 10..=120u16 {
            let details = detail_line(&app, &a_launch(), width, th);
            let text: Vec<char> = text_of(&details.line).chars().collect();
            let field = details
                .fields
                .iter()
                .find(|(f, _, _)| *f == BoxField::Worktree);
            let Some((x, w)) = details.branch else {
                let row: String = text.iter().collect();
                assert!(!row.contains("^T"), "{width}: no button, no ^T");
                assert!(field.is_none(), "{width}: no branch, no field");
                continue;
            };
            seen += 1;
            let under: String = text[usize::from(x)..usize::from(x + w)].iter().collect();
            assert!(
                under.starts_with("(worktree") && under.ends_with(" ^T"),
                "{width}: {under:?}"
            );
            let (_, fx, fw) = field.expect("a drawn branch is a field");
            assert!(
                *fx <= x && x + w <= fx + fw,
                "{width}: the branch sits inside its field"
            );
        }
        assert!(seen > 0, "some width has room for the branch");

        let pr = a_launch().with_pr(Some(crate::pull_request::PrLaunch {
            url: "https://github.com/o/r/pull/7".into(),
            head: "fix-nav".into(),
            number: 7,
        }));
        let details = detail_line(&app, &pr, 120, th);
        let text = text_of(&details.line);
        assert!(text.contains("worktree fix-nav"), "{text:?}");
        assert!(!text.contains("^T"), "{text:?}");
        assert_eq!(details.branch, None);
        assert!(
            !details
                .fields
                .iter()
                .any(|(f, _, _)| *f == BoxField::Worktree),
            "{text:?}"
        );
    }

    /// The branch a fresh worktree will be cut on is drawn in the green
    /// the box's frame turns, and cut before the project is when even the
    /// tight row runs short.
    #[test]
    fn a_fresh_worktree_branch_is_green_and_cut_first() {
        let th = Theme::default();
        let app = App::new();
        let mut launch = a_launch();
        launch.target = crate::quick_prompt::QuickTarget::NewWorktree {
            project: nebula_core::ProjectId("p".into()),
            branch: "yellow-fox-jumps-over-the-lazy-dog".into(),
        };
        let wide = detail_line(&app, &launch, 400, th).line;
        let branch = wide
            .spans
            .iter()
            .find(|s| s.content == "yellow-fox-jumps-over-the-lazy-dog")
            .expect("the branch, whole, on a wide row");
        assert_eq!(branch.style.fg, Some(th.ok));

        // Five columns short of the tight form — the airy one gave up
        // whole, as it does — so something has to be cut.
        let tight = detail_line(&app, &launch, wide.width() as u16 - 1, th).line;
        let short = tight.width() as u16 - 5;
        let text = text_of(&detail_line(&app, &launch, short, th).line);
        assert!(
            text.contains("(project gone)"),
            "the project kept whole: {text:?}"
        );
        assert!(text.contains("yellow-fox-"), "the branch cut: {text:?}");
        assert!(!text.contains("lazy-dog"), "the branch cut: {text:?}");
    }
    /// One checkout's BAND, with one session in it, for the rule tests.
    fn a_band(is_main: bool, branch: &str) -> crate::launcher::Band {
        use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, WorktreeId};
        crate::launcher::Band {
            worktree: WorktreeId("w1".into()),
            branch: branch.into(),
            is_main,
            solo: false,
            pr: None,
            cards: vec![crate::launcher::Card::Session(LauncherRow {
                agent: Agent {
                    parent: None,
                    profile: None,
                    id: AgentId("a1".into()),
                    worktree_id: WorktreeId("w1".into()),
                    name: "fix login".into(),
                    status: AgentStatus::Finished,
                    archived: false,
                    archived_at: 0,
                    unseen: false,
                    kind: AgentKind::Claude,
                    custom_harness: None,
                    model: None,
                    effort: None,
                    session_id: None,
                    cloud_session_id: None,
                    sort_order: 0,
                    status_changed_at: 0,
                    alive: true,
                    issue_url: None,
                    role: nebula_core::AgentRole::Worker,
                    recent_prompts: Vec::new(),
                },
                project: "nebula".into(),
                branch: branch.into(),
                pr: None,
            })],
        }
    }

    /// A band's rule drawn on one row `width` wide, as the buffer: a
    /// band the cursor is not on, with every card on its row.
    fn rule_row(app: &App, band: &crate::launcher::Band, width: u16) -> ratatui::buffer::Buffer {
        rule_row_as(
            app,
            band,
            width,
            BandRule {
                index: 0,
                on: false,
                lit: false,
                more: 0,
                expanded: false,
                list: false,
            },
        )
    }

    /// [`rule_row`] for a band drawn as `rule` says.
    fn rule_row_as(
        app: &App,
        band: &crate::launcher::Band,
        width: u16,
        rule: BandRule,
    ) -> ratatui::buffer::Buffer {
        let area = Rect::new(0, 0, width, 1);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|f| {
                draw_band_rule(f.buffer_mut(), app, area, band, rule);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The cells of a buffer's row `y` painted in `want`, as text.
    fn painted(buf: &ratatui::buffer::Buffer, y: u16, want: Color) -> String {
        (0..buf.area.width)
            .filter_map(|x| buf.cell((x, y)))
            .filter(|c| c.fg == want)
            .map(|c| c.symbol().to_string())
            .collect()
    }

    /// A buffer's row `y` as text.
    fn row_string(buf: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buf.area.width)
            .filter_map(|x| buf.cell((x, y)))
            .map(|c| c.symbol().to_string())
            .collect()
    }

    /// A band's rule names its checkout in the SCOPE COLOR — `⌂` root,
    /// `↳` worktree — glyph and branch both, and nothing else on the rule,
    /// so the two scopes sort a screenful of bands before a word is read.
    #[test]
    fn a_band_paints_its_checkout_in_the_scope_color() {
        let app = App::new();
        let th = app.chrome.theme;
        let root = rule_row(&app, &a_band(true, "main"), 44);
        let worktree = rule_row(&app, &a_band(false, "feat-x"), 44);
        assert_eq!(painted(&root, 0, th.root), "⌂ main");
        assert_eq!(painted(&worktree, 0, th.worktree), "↳ feat-x");
        // And neither wears the other's color anywhere on that row.
        assert_eq!(painted(&root, 0, th.worktree), "");
        assert_eq!(painted(&worktree, 0, th.root), "");
        // The cards under it say what runs in the checkout, not where.
        let text = row_string(&worktree, 0);
        assert!(text.contains("1 session"), "{text:?}");
    }

    /// The checkout's changed-file count rides right behind the branch on
    /// its band's rule, in the heads-up color, and on a rule too narrow
    /// for the word it keeps the number and drops `files` before the
    /// branch gives up more.
    #[test]
    fn a_band_counts_its_checkouts_changes_behind_the_branch() {
        use nebula_core::WorktreeId;
        let mut app = App::new();
        let th = app.chrome.theme;
        app.jobs.worktree_changes.insert(
            WorktreeId("w1".into()),
            (Some(3), std::time::Instant::now()),
        );
        let band = a_band(false, "feat-x");

        let buf = rule_row(&app, &band, 44);
        let text = row_string(&buf, 0);
        assert!(text.contains("↳ feat-x +3 files"), "{text:?}");
        assert_eq!(painted(&buf, 0, th.warn).trim(), "+3 files");

        let buf = rule_row(&app, &band, 24);
        let text = row_string(&buf, 0);
        assert_eq!(painted(&buf, 0, th.warn).trim(), "+3", "narrow: {text:?}");
    }

    /// The band the keys are on says so, and what Enter does: its rule
    /// opens on the CURSOR MARK `❯` in the accent where every other
    /// band's opens on `──`, and past its counts it spells the key —
    /// `z: see all 8` when cards hang past the row's edge, `z:
    /// open` when the row showed them all. A titled rule looks like a
    /// divider, and nothing about a divider says a key acts on it. With
    /// the cursor on the band but the keys elsewhere (up on the PROJECT
    /// TABS, down in the pane) the rule is the plain gray one again:
    /// Enter does something else from there.
    #[test]
    fn the_band_the_keys_are_on_marks_itself_and_says_what_enter_does() {
        let app = App::new();
        let th = app.chrome.theme;
        let key = crate::ui::key_hint(&app, crate::keymap::Action::FocusNext);
        let mut band = a_band(true, "main");
        let card = band.cards[0].clone();
        band.cards.extend(std::iter::repeat_n(card, 7));
        let lit = |more| BandRule {
            index: 0,
            on: true,
            lit: true,
            more,
            expanded: false,
            list: false,
        };

        let buf = rule_row_as(&app, &band, 96, lit(6));
        let text = row_string(&buf, 0);
        assert!(text.starts_with("❯  ⌂ main"), "{text:?}");
        assert!(
            text.contains(&format!("8 sessions  ▸ 6 more  {key}: see all 8 + ─")),
            "{text:?}"
        );
        // The mark and the key in the accent, the checkout still in its
        // own color and nothing else in it.
        let accent = painted(&buf, 0, th.accent);
        assert!(accent.starts_with('❯'), "{accent:?}");
        assert!(accent.contains(&key), "{accent:?}");
        assert_eq!(painted(&buf, 0, th.root), "⌂ main");

        // Every card on the row: nothing to see more of, so plain `expand`.
        let buf = rule_row_as(&app, &band, 96, lit(0));
        let text = row_string(&buf, 0);
        assert!(
            text.contains(&format!("8 sessions  {key}: expand + ─")),
            "{text:?}"
        );

        // The cursor's band with the keys elsewhere: bold branch, and
        // that is all — no mark, no hint, the count as on any band.
        let buf = rule_row_as(
            &app,
            &band,
            96,
            BandRule {
                index: 0,
                on: true,
                lit: false,
                more: 6,
                expanded: false,
                list: false,
            },
        );
        let text = row_string(&buf, 0);
        assert!(text.starts_with("── ⌂ main"), "keys elsewhere: {text:?}");
        assert!(text.contains("8 sessions  ▸ 6 more + ─"), "{text:?}");
        assert!(!text.contains(&key), "{text:?}");
        assert_eq!(painted(&buf, 0, th.accent), "", "{text:?}");
    }

    /// A rule too narrow to keep the branch legible beside the words
    /// drops the Enter hint before the branch gives way — the `❯` still
    /// says which band is selected, and the count still says what hangs
    /// past the edge. Give it the room and the words are back.
    #[test]
    fn a_narrow_rule_keeps_the_cursor_mark_and_drops_the_enter_hint() {
        let app = App::new();
        let key = crate::ui::key_hint(&app, crate::keymap::Action::FocusNext);
        let mut band = a_band(false, "feat-x");
        let card = band.cards[0].clone();
        band.cards.extend(std::iter::repeat_n(card, 2));
        let rule = BandRule {
            index: 0,
            on: true,
            lit: true,
            more: 2,
            expanded: false,
            list: false,
        };

        let buf = rule_row_as(&app, &band, 40, rule);
        let text = row_string(&buf, 0);
        assert!(text.starts_with("❯  ↳ feat-x"), "{text:?}");
        assert!(text.contains("3 sessions  ▸ 2 more + ─"), "{text:?}");
        assert!(!text.contains(&key), "{text:?}");

        let buf = rule_row_as(&app, &band, 72, rule);
        let text = row_string(&buf, 0);
        assert!(
            text.contains(&format!("3 sessions  ▸ 2 more  {key}: see all 3 + ─")),
            "{text:?}"
        );
    }

    /// A card's frame carries its status — red asking, blue done unread,
    /// the faint running yellow — and nothing else does: a read finish, a
    /// cold or archived card keeps the edge, and the focused card's accent
    /// outranks every status, so focus and running never look alike.
    #[test]
    fn a_cards_frame_answers_to_its_status_under_the_focus() {
        use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, WorktreeId};
        let base = Agent {
            parent: None,
            profile: None,
            id: AgentId("a1".into()),
            worktree_id: WorktreeId("w1".into()),
            name: "fix login".into(),
            status: AgentStatus::Finished,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: true,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        let th = Theme::by_name("amber");
        let mut app = App::new();
        app.chrome.theme = th;
        let frame = |agent: Agent, selected: bool, focused: bool| {
            let row = LauncherRow {
                agent,
                project: "nebula".into(),
                branch: "feat-x".into(),
                pr: None,
            };
            let area = Rect::new(0, 0, 40, crate::launcher::CARD_H);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height))
                    .unwrap();
            terminal
                .draw(|f| draw_one(f, &app, area, &row, selected, focused, th))
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            let corner = buf.cell((0, 0)).unwrap().fg;
            let side = buf.cell((0, 1)).unwrap().fg;
            assert_eq!(corner, side, "one color all the way round");
            corner
        };
        let with = |status: AgentStatus, unseen: bool| Agent {
            status,
            unseen,
            ..base.clone()
        };

        assert_eq!(
            frame(with(AgentStatus::NeedsFeedback, false), false, true),
            th.err
        );
        assert_eq!(
            frame(with(AgentStatus::Finished, true), false, true),
            th.done
        );
        assert_eq!(
            frame(with(AgentStatus::Running, false), false, true),
            th.warn
        );
        assert_eq!(
            frame(with(AgentStatus::Finished, false), false, true),
            th.edge
        );
        assert_eq!(frame(with(AgentStatus::Fresh, false), false, true), th.edge);
        let cold = Agent {
            parent: None,
            alive: false,
            ..with(AgentStatus::Running, false)
        };
        assert_eq!(frame(cold, false, true), th.edge);
        let archived = Agent {
            parent: None,
            archived: true,
            ..with(AgentStatus::NeedsFeedback, false)
        };
        assert_eq!(frame(archived, false, true), th.edge);

        // The cursor wins over every status, and keeps winning with the
        // keys off the grid: the outline still says which card the pane
        // reads while the wash has moved to the pane.
        assert_eq!(
            frame(with(AgentStatus::Running, false), true, true),
            th.accent
        );
        assert_eq!(
            frame(with(AgentStatus::NeedsFeedback, false), true, true),
            th.accent
        );
        assert_eq!(
            frame(with(AgentStatus::Running, false), true, false),
            th.accent
        );
        assert_eq!(
            frame(with(AgentStatus::Finished, false), true, false),
            th.accent
        );
        assert_ne!(th.warn, th.accent);
    }

    /// The cursor's card is raised out of the grid, frame and all: a heavy
    /// accent border no status frame takes, over the selection fill —
    /// `sel_bg` while the grid has the keys, `sel_bg_dim` while they are
    /// elsewhere. Every other card keeps its round, thin frame on the
    /// terminal's background.
    #[test]
    fn the_cursor_s_card_is_raised_with_a_heavy_frame_and_a_fill() {
        use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, WorktreeId};
        let row = LauncherRow {
            agent: Agent {
                parent: None,
                profile: None,
                id: AgentId("a1".into()),
                worktree_id: WorktreeId("w1".into()),
                name: "fix login".into(),
                status: AgentStatus::Finished,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: true,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            },
            project: "nebula".into(),
            branch: "feat-x".into(),
            pr: None,
        };
        let th = Theme::by_name("coral");
        let mut app = App::new();
        app.chrome.theme = th;
        let fill = |app: &App, selected: bool, focused: bool| {
            let area = Rect::new(0, 0, 40, crate::launcher::CARD_H);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height))
                    .unwrap();
            terminal
                .draw(|f| draw_one(f, app, area, &row, selected, focused, th))
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            let corner = buf.cell((0, 0)).unwrap();
            let inside = buf.cell((20, area.height - 2)).unwrap().bg;
            assert_eq!(corner.bg, inside, "one fill, frame and all");
            (corner.symbol().to_string(), inside)
        };
        assert_eq!(fill(&app, true, true), ("┏".into(), th.sel_bg));
        assert_eq!(fill(&app, true, false), ("┏".into(), th.sel_bg_dim));
        assert_eq!(fill(&app, false, true), ("╭".into(), Color::Reset));
        assert_eq!(fill(&app, false, false), ("╭".into(), Color::Reset));
    }

    /// HIGHLIGHT CURRENT CARD: the cursor's card is washed, very faintly,
    /// in its status color — breathing while it runs, still in the accent
    /// once nothing is going on — and keeps the wash while the pane has
    /// the keys.
    #[test]
    fn highlight_current_card_washes_the_card_faintly_in_its_status_color() {
        use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, WorktreeId};
        let row = LauncherRow {
            agent: Agent {
                parent: None,
                profile: None,
                id: AgentId("a1".into()),
                worktree_id: WorktreeId("w1".into()),
                name: "fix login".into(),
                status: AgentStatus::Running,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: true,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            },
            project: "nebula".into(),
            branch: "feat-x".into(),
            pr: None,
        };
        let th = Theme::by_name("coral");
        let mut app = App::new();
        app.chrome.theme = th;
        let draw = |app: &App, focused: bool| {
            let area = Rect::new(0, 0, 40, crate::launcher::CARD_H);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height))
                    .unwrap();
            terminal
                .draw(|f| draw_one(f, app, area, &row, true, focused, th))
                .unwrap();
            terminal.backend().buffer().clone()
        };

        // Off: the plain gray fill, dimmed off the grid.
        app.nav.focus = Focus::Terminal;
        let buf = draw(&app, false);
        assert_eq!(buf.cell((20, 1)).unwrap().bg, th.sel_bg_dim);

        // On: a faint wash of the running yellow, frame and all, while
        // the pane has the keys.
        app.launcher.highlight_current_card = true;
        let buf = draw(&app, false);
        let fill = buf.cell((20, 2)).unwrap().bg;
        assert_eq!(buf.cell((0, 0)).unwrap().bg, fill);
        let Color::Rgb(r, g, b) = fill else {
            panic!("{fill:?}")
        };
        assert!(r.max(g).max(b) <= 45, "faint: {fill:?}");
        assert!(r > b && g > b, "yellowish: {fill:?}");

        // It breathes between the floor and the peak.
        let levels: Vec<f32> = (0..TINT_BREATH).map(tint_level).collect();
        assert!(levels
            .iter()
            .all(|l| *l >= TINT_FLOOR - 1e-4 && *l <= TINT_PEAK + 1e-4));
        assert!(levels[TINT_BREATH / 2] > levels[0] + 0.1);
        assert_eq!(
            dim_toward_black(Color::Indexed(209), 0.5),
            Color::Rgb(128, 68, 48)
        );

        // A quiet card holds a still wash of the accent; the animations
        // off hold a live one still at its peak.
        let mut idle = row.clone();
        idle.agent.status = AgentStatus::Fresh;
        let idle_card = |app: &App| {
            let area = Rect::new(0, 0, 40, crate::launcher::CARD_H);
            let mut buf = Buffer::empty(area);
            draw_card(&mut buf, app, area, &idle, true, false, th, &mut None);
            buf.cell((20, 2)).unwrap().bg
        };
        assert_eq!(idle_card(&app), dim_toward_black(th.accent, TINT_STILL));
        app.chrome.animations = false;
        assert_eq!(
            draw(&app, false).cell((20, 2)).unwrap().bg,
            dim_toward_black(th.warn, TINT_PEAK)
        );

        // The PROJECT TABS holding the keys fade it further.
        app.launcher.launcher_tab_cursor = Some(nebula_core::ProjectId("p1".into()));
        assert_eq!(
            draw(&app, false).cell((20, 2)).unwrap().bg,
            dim_toward_black(th.warn, TINT_PEAK * 0.6)
        );
    }

    /// CARD LINE COUNTS: the lines behind the file count always follow it
    /// on the band's rule in the DIFF VIEWER's green and red — `+3 files
    /// +120 -45` — and yield after the word and before the branch as the
    /// rule narrows.
    #[test]
    fn card_line_counts_follow_the_file_count_in_green_and_red() {
        use nebula_core::WorktreeId;
        let mut app = App::new();
        let th = app.chrome.theme;
        app.jobs.worktree_changes.insert(
            WorktreeId("w1".into()),
            (Some(3), std::time::Instant::now()),
        );
        app.jobs.worktree_lines.insert(
            WorktreeId("w1".into()),
            crate::git_diff::LineChanges {
                added: 120,
                removed: 45,
            },
        );
        let band = a_band(false, "feat-x");
        let rule = |app: &App, width: u16| {
            let buf = rule_row(app, &band, width);
            (
                row_string(&buf, 0),
                painted(&buf, 0, th.warn),
                painted(&buf, 0, th.ok),
                painted(&buf, 0, th.err),
            )
        };

        let (text, warn, added, removed) = rule(&app, 60);
        assert!(text.contains("↳ feat-x +3 files +120 -45"), "{text:?}");
        assert_eq!(warn.trim(), "+3 files");
        assert_eq!(added.trim(), "+120");
        assert_eq!(removed.trim(), "-45");

        // One column more than before the rule's NEW BUTTON took one.
        let (text, warn, added, _) = rule(&app, 38);
        assert!(text.contains("↳ feat-x +3 +120 -45"), "{text:?}");
        assert_eq!((warn.trim(), added.trim()), ("+3", "+120"));

        let (text, warn, added, _) = rule(&app, 24);
        assert_eq!(warn.trim(), "+3", "narrowest: {text:?}");
        assert_eq!(added.trim(), "", "the lines go before the branch does");
    }

    /// An ARCHIVED card is the live card put away rather than the live card
    /// with a word added: its frame squares off, the round STATUS DOT gives
    /// way to the square ARCHIVED MARK, the name drops to muted, the SCOPE
    /// COLOR goes with the rest of the color, and the badge counts from when
    /// it was filed rather than from its last turn.
    #[test]
    fn an_archived_card_is_the_live_card_put_away() {
        use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, WorktreeId};

        fn card(archived: bool) -> LauncherRow {
            LauncherRow {
                agent: Agent {
                    parent: None,
                    profile: None,
                    id: AgentId("a1".into()),
                    worktree_id: WorktreeId("w1".into()),
                    name: "fix login".into(),
                    status: AgentStatus::Finished,
                    archived,
                    // Filed two hours ago, last turn a minute ago: the two
                    // badges cannot be mistaken for one another.
                    archived_at: crate::app::now_ms() - 2 * 3_600_000,
                    unseen: false,
                    kind: AgentKind::Claude,
                    custom_harness: None,
                    model: None,
                    effort: None,
                    session_id: None,
                    cloud_session_id: None,
                    sort_order: 0,
                    status_changed_at: crate::app::now_ms() - 60_000,
                    alive: true,
                    issue_url: None,
                    role: nebula_core::AgentRole::Worker,
                    recent_prompts: Vec::new(),
                },
                project: "nebula".into(),
                branch: "main".into(),
                pr: None,
            }
        }

        let th = Theme::default();
        let app = App::new();
        let area = Rect::new(0, 0, 44, crate::launcher::CARD_H);
        let drawn = |row: &LauncherRow| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height))
                    .unwrap();
            terminal
                .draw(|f| draw_one(f, &app, area, row, false, true, th))
                .unwrap();
            terminal.backend().buffer().clone()
        };
        // One row of the card as text, the frame and its air trimmed off
        // both ends.
        let line = |buf: &ratatui::buffer::Buffer, y: u16| -> String {
            let row: String = (0..area.width)
                .filter_map(|x| buf.cell((x, y)))
                .map(|c| c.symbol().to_string())
                .collect();
            row.trim_matches(|c| c == '\u{2502}' || c == ' ')
                .to_string()
        };
        // And one row's cells in a single color, for the rows whose whole
        // point is which color they are in.
        let colored = |buf: &ratatui::buffer::Buffer, y: u16, want: Color| -> String {
            (0..area.width)
                .filter_map(|x| buf.cell((x, y)))
                .filter(|c| c.fg == want)
                .map(|c| c.symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        };

        let live = drawn(&card(false));
        let gone = drawn(&card(true));

        // The frame: round corners live, square once filed \u{2014} the one part of
        // this a terminal with no color at all still says.
        assert!(
            line(&live, 0).starts_with('\u{256d}'),
            "{:?}",
            line(&live, 0)
        );
        assert!(
            line(&gone, 0).starts_with('\u{250c}'),
            "{:?}",
            line(&gone, 0)
        );

        // The mark stands where the dot stood, the name behind it starting
        // in the same column on both.
        assert!(
            line(&live, 1).starts_with("\u{25cf} fix login"),
            "{:?}",
            line(&live, 1)
        );
        assert!(
            line(&gone, 1).starts_with("\u{25aa} fix login"),
            "{:?}",
            line(&gone, 1)
        );

        // The archived name is muted, never the live card's text color.
        assert_eq!(colored(&gone, 1, th.muted), "fix login");
        assert_eq!(colored(&gone, 1, th.text), "");

        // The badge counts from the archiving, not from the last turn.
        assert!(line(&live, 1).ends_with("1m ago"), "{:?}", line(&live, 1));
        assert!(line(&gone, 1).ends_with("2h ago"), "{:?}", line(&gone, 1));

        // And the harness row is quiet on both: where the session runs is
        // its band's rule, not the card's.
        assert_eq!(colored(&live, 2, th.dim), "claude");
        assert_eq!(colored(&gone, 2, th.dim), "claude");
        assert_eq!(colored(&live, 2, th.root), "");
    }
}
