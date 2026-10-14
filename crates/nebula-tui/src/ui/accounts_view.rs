//! The ACCOUNTS OVERLAY's drawing (`crate::accounts_overlay`).

use super::*;
use crate::accounts_overlay::{age, AccountRow, AccountsView};
use nebula_core::usage::{self, Tone, UsageWindow};

/// Bar width in the overlay.
const BAR: usize = 10;

fn tone_fg(tone: Tone, th: Theme) -> Color {
    match tone {
        Tone::Ok => th.text,
        Tone::Warn => th.warn,
        Tone::Hot => th.err,
    }
}

/// `5h ▰▰▰▱▱▱▱▱▱▱ 31% in 2h 4m`, in the window's tone.
fn bar_spans(caption: &str, w: Option<&UsageWindow>, now: i64, th: Theme) -> Vec<Span<'static>> {
    let p = w.map(|w| w.percent_at(now));
    let fg = p.map_or(th.dim, |p| tone_fg(usage::tone(p), th));
    let caption = w
        .and_then(|w| w.label.clone())
        .unwrap_or_else(|| caption.to_string());
    let mut out = vec![
        Span::styled(format!("   {caption:<8}"), Style::default().fg(th.muted)),
        Span::styled(usage::block_bar(p, BAR), Style::default().fg(fg)),
        Span::styled(
            format!(" {:>4}", usage::pct_text(p)),
            Style::default().fg(fg),
        ),
    ];
    if let Some(at) = w.and_then(|w| w.resets_at) {
        out.push(Span::styled(
            format!("  resets {}", usage::resets_in(at, now)),
            Style::default().fg(th.dim),
        ));
    }
    out
}

/// The lines of one account: its name line, then its two bars.
pub(crate) fn row_lines(
    row: &AccountRow,
    selected: bool,
    now_ms: i64,
    th: Theme,
) -> Vec<Line<'static>> {
    let now = now_ms / 1000;
    let name_style = if selected {
        Style::default().fg(th.on_accent).bg(th.accent)
    } else if row.disabled() {
        Style::default().fg(th.dim)
    } else {
        Style::default().fg(th.text).add_modifier(Modifier::BOLD)
    };
    let mut head = vec![
        Span::styled(
            if row.default { " * " } else { "   " },
            Style::default().fg(th.accent),
        ),
        Span::styled(format!(" {} ", row.label()), name_style),
        Span::styled(format!("  {}", row.harness), Style::default().fg(th.muted)),
    ];
    if row.disabled() {
        head.push(Span::styled("  off", Style::default().fg(th.warn)));
    }
    let login = row.login_text();
    if !login.is_empty() {
        let fg = if login.starts_with("needs") {
            th.warn
        } else {
            th.dim
        };
        head.push(Span::styled(format!("  {login}"), Style::default().fg(fg)));
    }
    let [top, bottom] = row.usage.as_ref().map_or([None, None], |u| u.bars());
    let mut lines = vec![
        Line::from(head),
        Line::from(bar_spans("5h", top, now, th)),
        Line::from(bar_spans("weekly", bottom, now, th)),
    ];
    let source = match &row.usage {
        Some(u) => format!(
            "   {} \u{b7} {}{}",
            u.source.label(),
            age(u.at, now_ms),
            u.plan
                .as_deref()
                .map(|p| format!(" \u{b7} {p} plan"))
                .unwrap_or_default()
        ),
        None => "   no reading yet: it shows after a session's first turn".into(),
    };
    lines.push(Line::from(Span::styled(
        source,
        Style::default().fg(th.dim),
    )));
    lines
}

pub(super) fn draw(f: &mut Frame, app: &mut App, mut view: AccountsView) {
    let th = app.chrome.theme;
    let now = nebula_core::clock::now_ms();
    let height = (view.rows.len() as u16 * 5 + 4).clamp(8, f.area().height.saturating_sub(2));
    let area = centered_rect(f.area(), 76, height);
    let inner = render_modal_frame(f, area, " Accounts ", th);
    let mut lines = Vec::new();
    if view.rows.is_empty() {
        lines.push(Line::from(Span::styled(
            " No accounts yet: usage shows after a session's first turn, and kete profile add <name> --harness claude adds one.",
            Style::default().fg(th.dim),
        )));
    }
    // Keep the cursor's account on screen: each takes five lines.
    let per = 5usize;
    let fit = (inner.height as usize / per).max(1);
    let first = view.cursor.saturating_sub(fit - 1);
    for (i, row) in view.rows.iter().enumerate().skip(first).take(fit) {
        lines.extend(row_lines(row, i == view.cursor, now, th));
        lines.push(Line::from(""));
    }
    f.render_widget(Paragraph::new(lines), inner);
    view.area = area;
    if let Some(Overlay::Accounts(v)) = &mut app.modals.overlay {
        *v = view;
    }
}
