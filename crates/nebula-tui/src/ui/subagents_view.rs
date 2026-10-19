//! The SUBAGENTS' drawing (`crate::subagents` is the model,
//! `event_loop::desk` the keys): the `⑂ 1/3` chip on a session card's
//! bottom edge and in the PANE's header, and the SUBAGENTS OVERLAY — the
//! desktop overview's SESSION TREE, sessions nested under the session that
//! started them and each one's subagents under it.

use super::*;
use crate::subagents::{Row, SubagentsView, Tone};
use nebula_core::AgentId;
use ratatui::buffer::Buffer;

/// The overlay's width; its height follows the rows.
const WIDTH: u16 = 88;

/// The chip's glyph: a fork, for work split off the session.
const FORK: &str = "⑂";

/// A tone's color: the STATUS DOT's for the same state.
pub(crate) fn tone_color(tone: Tone, th: Theme) -> Color {
    match tone {
        Tone::Needs => th.err,
        Tone::Failed => th.special,
        Tone::Unread => th.done,
        Tone::Running => th.warn,
        Tone::Done => th.ok,
        Tone::Stopped | Tone::Idle => th.dim,
    }
}

/// `⑂ 1/3` — running of all — for a session with subagents; None
/// without any. In the running color while one runs, dim after.
fn chip(app: &App, id: &AgentId, th: Theme) -> Option<(String, Color)> {
    let (running, all) = app.desk.subagents.counts(id);
    if all == 0 {
        return None;
    }
    let fg = if running > 0 { th.warn } else { th.dim };
    Some((format!(" {FORK} {running}/{all} "), fg))
}

/// Where a card drawn at `area` puts its chip `width` cells wide: the
/// left end of its bottom edge, past the corner and a cell of rule — the
/// right end is the selected card's CARD BUTTONS'
/// (`launcher_view::card_buttons`), which leave the chip its room. None
/// on a card too narrow to keep its frame's corners clear.
fn chip_rect(area: Rect, width: u16) -> Option<Rect> {
    if area.height < 3 || area.width < width + 4 {
        return None;
    }
    Some(Rect {
        x: area.x + 2,
        y: area.bottom() - 1,
        width,
        height: 1,
    })
}

/// The cells the chip on `card` takes on its bottom edge, and the cell of
/// rule after it; 0 on a card with no subagents. What the CARD BUTTONS
/// keep clear of.
pub(crate) fn card_chip_width(app: &App, card: &crate::launcher::Card) -> usize {
    let crate::launcher::Card::Session(row) = card else {
        return 0;
    };
    chip(app, &row.agent.id, app.chrome.theme)
        .map_or(0, |(label, _)| Span::raw(label.as_str()).width() + 1)
}

/// The chip on the card of session `a` drawn at `area`: laid over its
/// bottom edge, underlined under the pointer.
pub(crate) fn draw_card_chip(
    buf: &mut Buffer,
    app: &App,
    area: Rect,
    a: &nebula_core::Agent,
    th: Theme,
) {
    let Some((label, fg)) = chip(app, &a.id, th) else {
        return;
    };
    let Some(r) = chip_rect(area, Span::raw(label.as_str()).width() as u16) else {
        return;
    };
    let mut style = Style::default().fg(fg);
    if app.launcher.hover_crumb == Some(HitTarget::SubagentChip(a.id.clone())) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    buf.set_string(r.x, r.y, label, style);
}

/// The click target of the chip on `card` placed at `placed`: only while
/// the card's bottom edge is on screen. Registered ahead of the card, so
/// the chip wins.
pub(crate) fn card_chip_hit(
    app: &App,
    card: &crate::launcher::Card,
    placed: crate::launcher::Placed,
) -> Option<(Rect, HitTarget)> {
    let crate::launcher::Card::Session(row) = card else {
        return None;
    };
    if placed.cut_bottom > 0 {
        return None;
    }
    let (label, _) = chip(app, &row.agent.id, app.chrome.theme)?;
    // The whole card's height decides whether it has room; its bottom
    // edge is on screen, so the chip's row is the placed rect's last.
    let whole = Rect {
        height: placed.rect.height + placed.cut_top,
        ..placed.rect
    };
    let r = chip_rect(whole, Span::raw(label.as_str()).width() as u16)?;
    let r = Rect {
        y: placed.rect.bottom().checked_sub(1)?,
        ..r
    };
    Some((r, HitTarget::SubagentChip(row.agent.id.clone())))
}

/// Most of a session card's prompt rows its SUBAGENT LINES take.
pub(crate) const CARD_SUBAGENT_LINES: usize = 2;

/// A session card's SUBAGENT LINES, under its prompt: each running
/// subagent (newest first) as `⑂ <task>   running 3m`, and the ended ones
/// either one by one (`done`, `failed`, `closed`) or, with the
/// `hide_done_subagents` SETTING, folded into one `⑂ 3 finished` count.
/// At most [`CARD_SUBAGENT_LINES`]; what does not fit is counted on the
/// last line, and `⇧S` (or the chip) opens the whole tree. Empty for a
/// session without subagents.
pub(crate) fn card_lines(
    app: &App,
    id: &AgentId,
    width: usize,
    quiet: bool,
    th: Theme,
) -> Vec<Vec<Span<'static>>> {
    let book = &app.desk.subagents;
    let all = book.of(id);
    if all.is_empty() || width < 12 {
        return Vec::new();
    }
    let hide_done = book.hide_done;
    let mut shown: Vec<&nebula_core::SubagentSummary> = all
        .iter()
        .filter(|s| !hide_done || s.status == nebula_core::SubagentStatus::Running)
        .collect();
    // Running first, then the most recent.
    shown.sort_by_key(|s| {
        (
            s.status != nebula_core::SubagentStatus::Running,
            std::cmp::Reverse(s.ended_at.unwrap_or(s.started_at)),
        )
    });
    let folded = if hide_done { book.ended(id) } else { 0 };
    let wanted = shown.len() + usize::from(folded > 0);
    let (rows, rest) = if wanted <= CARD_SUBAGENT_LINES {
        (shown.len(), 0)
    } else {
        let rows = CARD_SUBAGENT_LINES - 1;
        (rows, shown.len() - rows)
    };
    let now = crate::app::now_ms();
    let fg = |c: Color| if quiet { th.dim } else { c };
    let mut lines: Vec<Vec<Span<'static>>> = shown
        .iter()
        .take(rows)
        .map(|s| {
            let tone = crate::subagents::sub_tone(s);
            let tail = format!(
                "{} {}",
                crate::subagents::sub_word(s),
                crate::hosts::ago_label(now - s.ended_at.unwrap_or(s.started_at))
            )
            .trim_end()
            .to_string();
            let lead = format!("{FORK} ");
            let room = width.saturating_sub(lead.chars().count() + tail.chars().count() + 1);
            let label = truncate(&crate::subagents::sub_label(s), room);
            let pad = width.saturating_sub(
                lead.chars().count() + label.chars().count() + tail.chars().count(),
            );
            vec![
                Span::styled(lead, Style::default().fg(fg(tone_color(tone, th)))),
                Span::styled(label, Style::default().fg(fg(th.muted))),
                Span::raw(" ".repeat(pad)),
                Span::styled(tail, Style::default().fg(fg(tone_color(tone, th)))),
            ]
        })
        .collect();
    let mut summary = Vec::new();
    if rest > 0 {
        summary.push(format!("+{rest} more"));
    }
    if folded > 0 {
        summary.push(format!("{folded} finished"));
    }
    if !summary.is_empty() {
        let text = truncate(&format!("{FORK} {}", summary.join(" · ")), width);
        lines.push(vec![Span::styled(text, Style::default().fg(th.dim))]);
    }
    lines
}

/// The PANE header's chip for session `id`: `⑂ 2 of 3 subagents
/// running`, or `⑂ 3 subagents` once none runs. None without any.
pub(crate) fn pane_chip(app: &App, id: &AgentId) -> Option<Vec<Span<'static>>> {
    let th = app.chrome.theme;
    let (running, all) = app.desk.subagents.counts(id);
    if all == 0 {
        return None;
    }
    let noun = if all == 1 { "subagent" } else { "subagents" };
    let (text, fg) = if running > 0 {
        (format!("{FORK} {running} of {all} {noun} running"), th.warn)
    } else {
        (format!("{FORK} {all} {noun}"), th.muted)
    };
    let mut style = Style::default().fg(fg);
    if app.launcher.hover_crumb == Some(HitTarget::SubagentChip(id.clone())) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    // Its key beside it, the way the header's buttons carry theirs.
    Some(vec![
        Span::raw("  "),
        Span::styled(text, style),
        Span::styled(
            format!(" {}", key_hint(app, Action::Subagents)),
            Style::default().fg(th.dim),
        ),
    ])
}

/// The column a row at `depth` draws its fold mark at, inside the list
/// rect `area` — past the selection mark, two cells a level.
pub(crate) fn caret_x(area: Rect, depth: usize) -> u16 {
    area.x + 1 + (depth as u16).saturating_mul(2)
}

/// One row of the tree: the indent, the fold mark, the dot in the row's
/// tone, the name, what it is (dim), what a fold hides, its age.
fn row_spans(row: &Row, width: usize, now: i64, th: Theme) -> Vec<Span<'static>> {
    let n = &row.node;
    let indent = "  ".repeat(n.depth);
    let caret = if !row.has_kids() {
        "  "
    } else if row.expanded {
        "▾ "
    } else {
        "▸ "
    };
    let dot = if n.session { "● " } else { "◦ " };
    let color = tone_color(n.tone, th);
    let mut tail = String::new();
    if n.when > 0 {
        tail = crate::hosts::ago_label(now - n.when);
    }
    let hidden = if row.hidden > 0 {
        format!("  +{}", row.hidden)
    } else {
        String::new()
    };
    let word = if n.word.is_empty() {
        String::new()
    } else {
        format!("  {}", n.word)
    };
    let mut meta = if n.meta.is_empty() {
        String::new()
    } else {
        format!("  {}", n.meta)
    };
    let lead = indent.chars().count() + 4;
    let fixed = lead
        + word.chars().count()
        + meta.chars().count()
        + hidden.chars().count()
        + tail.chars().count()
        + 2;
    // A parent says what its nested sessions and subagents are doing, so a
    // folded one still shows that one needs you; dropped when the name
    // would have too little room left.
    let tally = crate::subagents::rollup(n).label();
    let fixed = if n.session
        && row.has_kids()
        && !tally.is_empty()
        && fixed + tally.chars().count() + 3 + 12 <= width
    {
        meta.push_str(&format!(" · {tally}"));
        fixed + tally.chars().count() + 3
    } else {
        fixed
    };
    let label = truncate(&n.label, width.saturating_sub(fixed).max(8));
    let name_style = if n.session {
        Style::default().fg(th.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.muted)
    };
    let mut spans = vec![
        Span::raw(indent),
        Span::styled(caret, Style::default().fg(th.muted)),
        Span::styled(dot, Style::default().fg(color)),
        Span::styled(label.clone(), name_style),
        Span::styled(word.clone(), Style::default().fg(color)),
        Span::styled(meta.clone(), Style::default().fg(th.dim)),
        Span::styled(hidden.clone(), Style::default().fg(th.muted)),
    ];
    let used = lead
        + label.chars().count()
        + word.chars().count()
        + meta.chars().count()
        + hidden.chars().count();
    if !tail.is_empty() && used + tail.chars().count() + 1 < width {
        spans.push(Span::raw(" ".repeat(width - used - tail.chars().count())));
        spans.push(Span::styled(tail, Style::default().fg(th.dim)));
    }
    spans
}

pub(super) fn draw(f: &mut Frame, app: &mut App, mut view: SubagentsView) {
    let th = app.chrome.theme;
    let now = crate::app::now_ms();
    let ceiling = f.area().height.saturating_sub(2);
    let height = (view.rows.len() as u16 + 4).clamp(8, 30).min(ceiling);
    let area = centered_rect(f.area(), WIDTH, height);
    let scope = match &view.project {
        Some(id) => app
            .tree
            .projects
            .iter()
            .find(|p| &p.id == id)
            .map_or_else(|| "project".to_string(), |p| p.name.clone()),
        None => "all projects".to_string(),
    };
    let inner = render_modal_frame(f, area, format!(" Sessions · {scope} "), th);
    // The first row: what the tree holds, and Tab's other scope.
    let sessions = view.rows.iter().filter(|r| r.node.session).count();
    let running: usize = view
        .rows
        .iter()
        .filter(|r| r.node.session)
        .map(|r| app.desk.subagents.counts(&r.node.opens).0)
        .sum();
    if let Some(head) = row_rect(inner, 0) {
        let other = if view.project.is_some() {
            "Tab: every project"
        } else {
            "Tab: this project"
        };
        let line = Line::from(vec![
            Span::styled(
                format!(
                    " {sessions} session{} · {running} subagent{} running",
                    if sessions == 1 { "" } else { "s" },
                    if running == 1 { "" } else { "s" },
                ),
                Style::default().fg(th.muted),
            ),
            Span::styled(format!("   {other}"), Style::default().fg(th.dim)),
        ]);
        f.render_widget(Paragraph::new(line), head);
    }
    let list = below_first_row(inner);
    if view.rows.is_empty() {
        empty_list_row(
            f,
            list,
            " no live sessions here: Tab shows every project",
            th,
        );
    }
    let fit = usize::from(list.height.max(1));
    // The last frame's window, moved only as far as the cursor needs.
    let mut start = view.scroll.min(view.cursor);
    if view.cursor >= start + fit {
        start = view.cursor + 1 - fit;
    }
    let width = usize::from(list.width).saturating_sub(2);
    for (i, row) in view.rows.iter().enumerate().skip(start).take(fit) {
        let Some(r) = row_rect(list, i - start) else {
            break;
        };
        render_row(
            f,
            r,
            row_spans(row, width, now, th),
            i == view.cursor,
            true,
            th,
        );
    }
    view.area = area;
    view.list_area = list;
    view.scroll = start;
    if let Some(Overlay::Subagents(v)) = &mut app.modals.overlay {
        v.area = view.area;
        v.list_area = view.list_area;
        v.scroll = view.scroll;
    }
}
