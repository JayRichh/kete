//! Drawing the GAUNTLET VIEW overlay (K5): the kind tabs, the filter line,
//! the rows (tone glyph and word, id, by, at, the note's first line) or the
//! selected verdict's detail, and the keys on the last row.

use super::*;
use crate::gauntlet_view::GauntletView;
use nebula_core::studio::gauntlet::{tone, Tone};
use nebula_core::studio::verdicts::Verdict;
use ratatui::widgets::Wrap;

/// A glyph beside the colour, so a tone never rests on colour alone.
pub(crate) fn glyph(t: Tone) -> &'static str {
    match t {
        Tone::Done => "✓",
        Tone::Failed => "✕",
        Tone::NeedsYou => "●",
        Tone::Plain => "·",
    }
}

/// The subagents' colours: needs you is the error (ember) colour, and
/// only it.
fn tone_color(t: Tone, th: Theme) -> Color {
    match t {
        Tone::Done => th.ok,
        Tone::Failed => th.special,
        Tone::NeedsYou => th.err,
        Tone::Plain => th.muted,
    }
}

/// `● split`, in its tone.
fn word_span(v: &Verdict, width: usize, th: Theme) -> Span<'static> {
    let t = tone(&v.verdict);
    let word = if v.verdict.is_empty() {
        "-"
    } else {
        &v.verdict
    };
    Span::styled(
        format!(
            "{:<width$}",
            fit_to_width(&format!("{} {word}", glyph(t)), width)
        ),
        Style::default().fg(tone_color(t, th)),
    )
}

fn pad(s: &str, width: usize) -> String {
    format!("{:<width$}", fit_to_width(s, width))
}

/// One verdict row, `width` cells wide.
fn row_line(v: &Verdict, width: usize, selected: bool, th: Theme) -> Line<'static> {
    let id_w = v.id.chars().count().clamp(8, 28);
    let note = v
        .note
        .as_deref()
        .and_then(|n| n.lines().next())
        .unwrap_or("");
    let mut spans = vec![
        Span::raw(" "),
        word_span(v, 13, th),
        Span::raw(pad(&v.id, id_w + 1)),
    ];
    let mut used = 1 + 13 + id_w + 1;
    // `by` goes first when the row is narrow; `at` always stays.
    if width >= 90 {
        spans.push(Span::styled(
            pad(v.by.as_deref().unwrap_or(""), 16),
            Style::default().fg(th.muted),
        ));
        used += 16;
    }
    spans.push(Span::styled(
        pad(v.at.as_deref().unwrap_or(""), 11),
        Style::default().fg(th.muted),
    ));
    used += 11;
    spans.push(Span::styled(
        pad(note, width.saturating_sub(used)),
        Style::default().fg(th.dim),
    ));
    let mut line = Line::from(spans);
    if selected {
        line = line.style(Style::default().bg(th.sel_bg));
    }
    line
}

fn bytes(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)),
    }
}

/// The selected verdict, whole.
fn detail_lines(view: &GauntletView, v: &Verdict, th: Theme) -> Vec<Line<'static>> {
    let dim = Style::default().fg(th.dim);
    let field = |name: &str, value: String| {
        Line::from(vec![
            Span::styled(format!(" {name:<11}"), dim),
            Span::raw(value),
        ])
    };
    let root = view.root.clone().unwrap_or_default();
    let mut lines = vec![Line::from(vec![
        Span::raw(" "),
        word_span(v, v.verdict.chars().count() + 3, th)
            .patch_style(Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(v.id.clone(), Style::default().add_modifier(Modifier::BOLD)),
    ])];
    let department = view.kind().and_then(|k| k.department.clone());
    lines.push(field(
        "kind",
        match department {
            Some(d) => format!("{} ({d})", v.kind),
            None => v.kind.clone(),
        },
    ));
    if let Some(by) = &v.by {
        lines.push(field("by", by.clone()));
    }
    if let Some(at) = &v.at {
        lines.push(field("at", at.clone()));
    }
    lines.push(field(
        "file",
        nebula_core::studio::gauntlet::repo_rel(&root, &v.path),
    ));
    if let Some(r) = &v.reconciled {
        lines.push(field(
            "reconciled",
            nebula_core::studio::gauntlet::repo_rel(&root, r),
        ));
    }
    if !v.tags.is_empty() {
        lines.push(field("tags", v.tags.join(", ")));
    }
    if let Some(url) = view.studio_link() {
        lines.push(field("studio", format!("o opens {url}")));
    }
    if let Some(note) = &v.note {
        lines.push(Line::raw(""));
        lines.extend(note.lines().map(|l| Line::raw(format!(" {l}"))));
    }
    for id in &v.evidence {
        lines.push(Line::raw(""));
        lines.push(Line::styled(format!(" evidence {id}"), dim));
        match view.evidence.get(id) {
            Some(Some(entries)) if entries.is_empty() => {
                lines.push(Line::styled("   nothing there", dim));
            }
            Some(Some(entries)) => lines.extend(entries.iter().map(|e| {
                let size = if e.is_dir {
                    "folder".to_string()
                } else {
                    bytes(e.bytes)
                };
                Line::from(vec![
                    Span::raw(format!(
                        "   {}{}  ",
                        e.name,
                        if e.is_dir { "/" } else { "" }
                    )),
                    Span::styled(size, dim),
                ])
            })),
            _ => lines.push(Line::styled("   reading…", dim)),
        }
    }
    lines
}

/// One dim line on the body's first row: why there are no rows.
fn say(f: &mut Frame, body: Rect, text: &str, th: Theme) {
    empty_list_row(f, body, &format!(" {text}"), th);
}

/// Draw the gauntlet overlay over `area`.
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let jobs = app.jobs.view_jobs.clone();
    let view = match &mut app.modals.overlay {
        Some(Overlay::Gauntlet(v)) => {
            crate::gauntlet_view::sync(jobs, v);
            v.clone()
        }
        _ => return,
    };
    let th = app.chrome.theme;
    let rect = centered_rect_pct(area, 92, 90);
    let title = if view.name.is_empty() {
        " Gauntlet ".to_string()
    } else {
        format!(" Gauntlet · {} ", view.name)
    };
    let inner = render_modal_frame(f, rect, title, th);
    let width = inner.width as usize;
    let body = Rect {
        y: inner.y.saturating_add(3),
        height: inner.height.saturating_sub(4),
        ..inner
    };
    let mut tab_hits = Vec::new();
    let mut list_area = Rect::default();
    let mut scroll = 0;

    match view.data.as_deref() {
        _ if view.root.is_none() => say(
            f,
            inner,
            "No repo here has studio mode: add .kete/studio.toml to one.",
            th,
        ),
        None => say(f, inner, "Reading verdicts…", th),
        Some(d) if d.error.is_some() => say(f, inner, d.error.as_deref().unwrap_or(""), th),
        Some(d) if d.kinds.is_empty() => {
            let at = d
                .manifest
                .as_ref()
                .and_then(|m| m.signoffs.clone())
                .unwrap_or_else(|| "signoffs (not set in .kete/studio.toml)".into());
            say(f, inner, &format!("No verdict folders in {at}."), th);
        }
        Some(d) => {
            // ---- kind tabs and their rule ----
            let labels: Vec<String> = d
                .kinds
                .iter()
                .map(|k| format!("{} {}", k.kind, k.verdicts.len()))
                .collect();
            let (strip, hits) = tab_strip(
                inner.x,
                labels.iter().map(|l| (l.as_str(), None)),
                view.tab,
                false,
                th,
            );
            tab_hits = hits;
            f.render_widget(
                Paragraph::new(vec![Line::from(strip), strip_rule(inner.width, th)]),
                Rect {
                    height: inner.height.min(2),
                    ..inner
                },
            );

            // ---- the filter line ----
            let total = view.kind().map_or(0, |k| k.verdicts.len());
            let count = format!("{} of {total} ", view.list.len());
            let query = if view.filtering {
                format!(" / {}▏", view.list.query.as_str())
            } else if view.list.query.is_empty() {
                " / to filter".to_string()
            } else {
                format!(" / {}", view.list.query.as_str())
            };
            let gap = width.saturating_sub(query.chars().count() + count.chars().count());
            if let Some(r) = row_rect(inner, 2) {
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            query,
                            Style::default().fg(if view.filtering { th.text } else { th.dim }),
                        ),
                        Span::raw(" ".repeat(gap)),
                        Span::styled(count, Style::default().fg(th.dim)),
                    ])),
                    r,
                );
            }

            // ---- the rows, or the detail ----
            match (view.detail, view.selected()) {
                (true, Some(v)) => {
                    f.render_widget(
                        Paragraph::new(detail_lines(&view, v, th))
                            .wrap(Wrap { trim: false })
                            .scroll((view.detail_scroll, 0)),
                        body,
                    );
                }
                _ if view.list.is_empty() => say(f, body, "Nothing matches.", th),
                _ => {
                    let height = body.height as usize;
                    scroll = view.list.window_start(height);
                    let kind = view.kind();
                    let lines: Vec<Line> = view
                        .list
                        .matches
                        .iter()
                        .enumerate()
                        .skip(scroll)
                        .take(height)
                        .filter_map(|(row, m)| {
                            let v = kind?.verdicts.get(*view.list.items.get(m.item)?)?;
                            Some(row_line(v, width, row == view.list.cursor, th))
                        })
                        .collect();
                    f.render_widget(Paragraph::new(lines), body);
                    list_area = body;
                }
            }
        }
    }

    // ---- keys on the last row ----
    let hint = if view.filtering {
        " type to filter · Enter keep · Esc clear"
    } else if view.detail {
        " ↑↓ verdict · PgUp PgDn scroll · o Studio · Enter back · Esc back"
    } else {
        " Tab kind · Enter detail · o Studio · / filter · r reload · Esc close"
    };
    if let Some(r) = row_rect(inner, inner.height.saturating_sub(1) as usize) {
        f.render_widget(
            Paragraph::new(Span::styled(
                truncate(hint, width),
                Style::default().fg(th.dim),
            )),
            r,
        );
    }

    if let Some(Overlay::Gauntlet(v)) = &mut app.modals.overlay {
        v.area = rect;
        v.tab_hits = tab_hits;
        v.tabs_y = inner.y;
        v.list.list_area = list_area;
        v.list.scroll = scroll;
        v.page = body.height;
    }
}
