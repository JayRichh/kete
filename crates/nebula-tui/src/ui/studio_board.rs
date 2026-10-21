//! Drawing the STUDIO BOARD overlay (K4): the queue's header line, then
//! What needs you and one row per department (name, queue, lead, runners,
//! last verdict, blocked on Jay, today's spend), an expanded department's
//! notes, sessions and verdict under it, and the keys on the last row.

use super::*;
use crate::studio_board::{rows, sync, Row, MAX_NOTES_SHOWN, NEEDS_KEY};
use nebula_core::studio::board::{
    budget_level, Board, BudgetLevel, Column, NeedsYou, SessionCell, SpendSum,
};

/// A glyph beside the dot's colour, so a state never rests on colour alone.
fn status_glyph(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Fresh | AgentStatus::Disconnected => "○",
        AgentStatus::Running => "◐",
        AgentStatus::Finished => "●",
        AgentStatus::NeedsFeedback => "!",
        AgentStatus::Terminated => "✕",
    }
}

fn status_word(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::NeedsFeedback => "needs you",
        AgentStatus::Running => "running",
        AgentStatus::Finished => "done",
        AgentStatus::Terminated | AgentStatus::Disconnected => "stopped",
        AgentStatus::Fresh => "idle",
    }
}

/// `s` in `width` cells, left-aligned, a space kept before the next cell.
fn pad(s: &str, width: usize) -> String {
    format!(
        "{:<width$}",
        crate::ui::rows::fit_to_width(s, width.saturating_sub(1))
    )
}

fn rpad(s: &str, width: usize) -> String {
    format!(
        "{:>w$} ",
        crate::ui::rows::fit_to_width(s, width.saturating_sub(1)),
        w = width.saturating_sub(1)
    )
}

fn money(s: SpendSum) -> String {
    if s.tokens == 0 && s.cost_usd == 0.0 {
        String::new()
    } else {
        format!("${:.2}", s.cost_usd)
    }
}

fn tokens(n: u64) -> String {
    match n {
        0 => String::new(),
        n if n >= 1_000_000 => format!("{:.1}M tok", n as f64 / 1e6),
        n if n >= 1_000 => format!("{}k tok", n / 1_000),
        n => format!("{n} tok"),
    }
}

/// The date part of an `at` stamp.
fn day(at: Option<&str>) -> String {
    at.map(|a| a.chars().take(10).collect()).unwrap_or_default()
}

/// The widths of a department row's cells after the name.
struct Cols {
    name: usize,
    tokens: bool,
}

const MARK: usize = 3;
const QUEUE: usize = 6;
const LEAD: usize = 15;
const RUN: usize = 4;
const VERDICT: usize = 11;
const JAY: usize = 4;
const SPEND: usize = 8;
const TOKENS: usize = 10;

fn cols(width: usize) -> Cols {
    let tokens = width >= 100;
    let fixed = MARK + QUEUE + LEAD + RUN + VERDICT + JAY + SPEND + if tokens { TOKENS } else { 0 };
    Cols {
        name: width.saturating_sub(fixed).max(8),
        tokens,
    }
}

fn titles_line(c: &Cols, th: Theme) -> Line<'static> {
    let mut s = format!(
        "{}{}{}{}{}{}{}{}",
        " ".repeat(MARK),
        pad("Department", c.name),
        pad("Queue", QUEUE),
        pad("Lead", LEAD),
        pad("Run", RUN),
        pad("Verdict", VERDICT),
        pad("Jay", JAY),
        rpad("Today", SPEND),
    );
    if c.tokens {
        s.push_str(&rpad("Tokens", TOKENS));
    }
    Line::styled(s, Style::default().fg(th.dim))
}

fn dept_line(col: &Column, open: bool, c: &Cols, th: Theme) -> Line<'static> {
    let dim = Style::default().fg(th.dim);
    let mark = if open { " ▾ " } else { " ▸ " };
    let mut spans = vec![
        Span::styled(mark, dim),
        Span::styled(
            pad(&col.department.label, c.name),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    // Unassigned holds sessions only: no queue and no lead to speak of.
    let unassigned = col.department.slug == nebula_core::studio::board::UNASSIGNED;
    if unassigned {
        spans.push(Span::raw(" ".repeat(QUEUE + LEAD)));
    } else {
        spans.push(Span::raw(pad(&col.queue.len().to_string(), QUEUE)));
    }
    match &col.lead {
        _ if unassigned => {}
        Some(lead) => spans.push(Span::styled(
            pad(
                &format!("{} {}", status_glyph(lead.status), lead.label),
                LEAD,
            ),
            Style::default().fg(crate::ui::rows::status_color(Some(lead.status), false, th)),
        )),
        None => spans.push(Span::styled(pad("· no lead", LEAD), dim)),
    }
    let runners = if col.runners.is_empty() {
        String::new()
    } else {
        format!("+{}", col.runners.len())
    };
    spans.push(Span::raw(pad(&runners, RUN)));
    spans.push(Span::styled(
        pad(
            col.last_verdict.as_ref().map_or("", |v| {
                if v.verdict.is_empty() {
                    "-"
                } else {
                    &v.verdict
                }
            }),
            VERDICT,
        ),
        Style::default().fg(th.muted),
    ));
    // Ember means needs you, and only it.
    if col.blocked > 0 {
        spans.push(Span::styled(
            pad(&format!("!{}", col.blocked), JAY),
            Style::default().fg(th.err).add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::raw(" ".repeat(JAY)));
    }
    spans.push(Span::raw(rpad(&money(col.spend), SPEND)));
    if c.tokens {
        spans.push(Span::styled(rpad(&tokens(col.spend.tokens), TOKENS), dim));
    }
    Line::from(spans)
}

fn budget_span(cell: &SessionCell, th: Theme) -> Span<'static> {
    let Some(pct) = cell.context_percent_of_budget else {
        return Span::raw("");
    };
    let style = match budget_level(pct) {
        BudgetLevel::Ok => Style::default().fg(th.muted),
        BudgetLevel::Amber => Style::default().fg(th.warn),
        BudgetLevel::Red => Style::default().fg(th.special).add_modifier(Modifier::BOLD),
    };
    Span::styled(format!("ctx {pct}%"), style)
}

fn session_line(cell: &SessionCell, lead: bool, th: Theme) -> Line<'static> {
    let dim = Style::default().fg(th.dim);
    let colour = crate::ui::rows::status_color(Some(cell.status), false, th);
    let mut spans = vec![
        Span::raw("     "),
        Span::styled(
            format!("{} ", status_glyph(cell.status)),
            Style::default().fg(colour),
        ),
        Span::raw(cell.label.clone()),
    ];
    if lead {
        spans.push(Span::styled(" (lead)", dim));
    }
    let word = status_word(cell.status);
    spans.push(Span::styled(
        format!("  {word}"),
        if cell.status == AgentStatus::NeedsFeedback {
            Style::default().fg(th.err)
        } else {
            dim
        },
    ));
    for fact in [&cell.model, &cell.profile].into_iter().flatten() {
        spans.push(Span::styled(format!("  {fact}"), dim));
    }
    spans.push(Span::raw("  "));
    spans.push(budget_span(cell, th));
    Line::from(spans)
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

fn needs_header(board: &Board, open: bool, th: Theme) -> Line<'static> {
    let count = |f: fn(&NeedsYou) -> bool| board.needs_you.iter().filter(|n| f(n)).count();
    let mut parts = Vec::new();
    for (n, one) in [
        (count(|n| matches!(n, NeedsYou::Session(_))), "session"),
        (count(|n| matches!(n, NeedsYou::Note(_))), "note"),
        (count(|n| matches!(n, NeedsYou::Refusal(_))), "refusal"),
    ] {
        if n > 0 {
            parts.push(plural(n, one));
        }
    }
    if board
        .needs_you
        .iter()
        .any(|n| matches!(n, NeedsYou::Renewal(_)))
    {
        parts.push("renewal failed".into());
    }
    let ember = Style::default().fg(th.err);
    Line::from(vec![
        Span::styled(if open { " ▾ " } else { " ▸ " }, ember),
        Span::styled("What needs you  ", ember.add_modifier(Modifier::BOLD)),
        Span::styled(parts.join(" · "), ember),
    ])
}

fn need_line(n: &NeedsYou, th: Theme) -> Line<'static> {
    let dim = Style::default().fg(th.dim);
    match n {
        NeedsYou::Session(c) => session_line(c, false, th),
        NeedsYou::Note(n) => Line::from(vec![
            Span::styled("     note  ", dim),
            Span::raw(n.title.clone()),
            Span::styled(
                n.about
                    .as_deref()
                    .map(|a| format!("  {a}"))
                    .unwrap_or_default(),
                dim,
            ),
        ]),
        NeedsYou::Refusal(r) => Line::from(vec![
            Span::styled("     refused  ", dim),
            Span::raw(match &r.role {
                Some(role) => format!("{role}: {}", r.reason),
                None => r.reason.clone(),
            }),
        ]),
        NeedsYou::Renewal(r) => Line::from(vec![
            Span::styled("     renewal failed  ", dim),
            Span::raw(r.reason.clone()),
        ]),
    }
}

/// The line `row` shows.
fn row_line(
    board: &Board,
    row: &Row,
    expanded: &std::collections::BTreeSet<String>,
    c: &Cols,
    th: Theme,
) -> Line<'static> {
    let dim = Style::default().fg(th.dim);
    let col = |slug: &str| board.columns.iter().find(|x| x.department.slug == slug);
    match row {
        Row::NeedsYou => needs_header(board, expanded.contains(NEEDS_KEY), th),
        Row::Need(i) => board
            .needs_you
            .get(*i)
            .map_or_else(Line::default, |n| need_line(n, th)),
        Row::Dept(slug) => col(slug).map_or_else(Line::default, |x| {
            dept_line(x, expanded.contains(slug), c, th)
        }),
        Row::Note(slug, id) => col(slug)
            .and_then(|x| x.queue.iter().take(MAX_NOTES_SHOWN).find(|n| &n.id == id))
            .map_or_else(Line::default, |n| {
                let high = n.priority == nebula_core::studio::inbox::Priority::High;
                Line::from(vec![
                    Span::styled("     · ", dim),
                    Span::styled(
                        n.title.clone(),
                        if high {
                            Style::default().add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                    Span::styled(
                        format!("  {}  {}", n.priority.as_str(), day(n.at.as_deref())),
                        dim,
                    ),
                ])
            }),
        Row::Session(slug, agent) => col(slug).map_or_else(Line::default, |x| {
            let lead = x.lead.as_ref().filter(|l| &l.agent == agent);
            lead.or_else(|| x.runners.iter().find(|r| &r.agent == agent))
                .map_or_else(Line::default, |cell| session_line(cell, lead.is_some(), th))
        }),
        Row::Verdict(slug) => {
            col(slug)
                .and_then(|x| x.last_verdict.as_ref())
                .map_or_else(Line::default, |v| {
                    Line::from(vec![
                        Span::styled("     verdict  ", dim),
                        Span::raw(format!("{}  {}", v.verdict, v.id)),
                        Span::styled(format!("  {}", day(v.at.as_deref())), dim),
                    ])
                })
        }
    }
}

/// One dim line on the body's first row: why there are no rows.
fn say(f: &mut Frame, body: Rect, text: &str, th: Theme) {
    empty_list_row(f, body, &format!(" {text}"), th);
}

/// Draw the board overlay over `area`.
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let jobs = app.jobs.view_jobs.clone();
    let view = match &mut app.modals.overlay {
        Some(Overlay::StudioBoard(v)) => {
            sync(jobs, v);
            v.clone()
        }
        _ => return,
    };
    if view.root.is_none() {
        let rect = empty_state(
            f,
            app,
            area,
            " Studio board ",
            "No repo here has studio mode: add .kete/studio.toml to one.",
        );
        if let Some(Overlay::StudioBoard(v)) = &mut app.modals.overlay {
            v.area = rect;
            v.list_area = Rect::default();
        }
        return;
    }
    let th = app.chrome.theme;
    let rect = centered_rect_pct(area, 96, 90);
    let title = if view.name.is_empty() {
        " Studio board ".to_string()
    } else {
        format!(" Studio board · {} ", view.name)
    };
    let inner = render_modal_frame(f, rect, title, th);
    let width = inner.width as usize;
    let c = cols(width);
    let body = Rect {
        y: inner.y.saturating_add(2),
        height: inner.height.saturating_sub(3),
        ..inner
    };
    let mut list_area = Rect::default();
    let mut scroll = view.scroll;
    let mut cursor = view.cursor;

    if let Some(r) = row_rect(inner, 0) {
        let header = crate::studio_queue_view::header_line(app, view.repo.as_ref());
        f.render_widget(Paragraph::new(header), r);
    }
    let files = view.files.as_deref();
    let board = crate::studio_board::board_of(app, &view);
    match (files, &board) {
        (None, _) | (_, None) => say(f, body, "Reading the board…", th),
        (Some(d), _) if !d.manifest => say(f, body, "This repo has no .kete/studio.toml.", th),
        (Some(d), _) if d.error.is_some() => say(f, body, d.error.as_deref().unwrap_or(""), th),
        (_, Some(b)) if b.columns.is_empty() && b.needs_you.is_empty() => say(
            f,
            body,
            "No departments: the manifest's departments file has no table.",
            th,
        ),
        (_, Some(b)) => {
            if let Some(r) = row_rect(inner, 1) {
                f.render_widget(Paragraph::new(titles_line(&c, th)), r);
            }
            let all = rows(b, &view.expanded);
            let height = (body.height as usize).max(1);
            cursor = cursor.min(all.len().saturating_sub(1));
            if cursor < scroll {
                scroll = cursor;
            } else if cursor >= scroll + height {
                scroll = cursor + 1 - height;
            }
            scroll = scroll.min(all.len().saturating_sub(height));
            let lines: Vec<Line> = all
                .iter()
                .enumerate()
                .skip(scroll)
                .take(height)
                .map(|(i, row)| {
                    let line = row_line(b, row, &view.expanded, &c, th);
                    if i == cursor {
                        line.style(Style::default().bg(th.sel_bg))
                    } else {
                        line
                    }
                })
                .collect();
            f.render_widget(Paragraph::new(lines), body);
            list_area = body;
        }
    }

    let hint = " Enter open · l lead · v verdict · n note · m menu · r reload · Esc close";
    if let Some(r) = row_rect(inner, inner.height.saturating_sub(1) as usize) {
        f.render_widget(
            Paragraph::new(Span::styled(
                truncate(hint, width),
                Style::default().fg(th.dim),
            )),
            r,
        );
    }

    if let Some(Overlay::StudioBoard(v)) = &mut app.modals.overlay {
        v.area = rect;
        v.list_area = list_area;
        v.scroll = scroll;
        v.cursor = cursor;
    }
}
