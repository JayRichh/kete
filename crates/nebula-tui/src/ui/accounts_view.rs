//! The ACCOUNTS OVERLAY's drawing (`crate::accounts_overlay`): the
//! Accounts view, and the Wake view (`crate::wake_tab`) beside it.

use super::*;
use crate::accounts_overlay::{
    age, AccountForm, AccountRow, AccountsHit, AccountsView, FormKind, Mode, Tab,
};
use crate::wake_tab::{self, WakeTab};
use crossterm::event::KeyCode;
use nebula_core::usage::{self, Tone, UsageWindow};

/// Bar width in the overlay.
const BAR: usize = 10;

fn tone_fg(tone: Tone, th: Theme) -> Color {
    match tone {
        Tone::Ok => th.text,
        Tone::Watch => th.accent,
        Tone::Warn => th.warn,
        Tone::Hot => th.err,
    }
}

/// `5h ▰▰▰▱▱▱▱▱▱▱ 31% resets 9:50 pm in 2 h 4 m`, in the window's tone; dim, with the
/// reading's age inline, once it is stale (`UsageWindow::stale_at`).
fn bar_spans(caption: &str, w: Option<&UsageWindow>, now: i64, th: Theme) -> Vec<Span<'static>> {
    let p = w.and_then(|w| w.shown_at(now));
    let stale = w.is_some_and(|w| w.stale_at(now));
    let fg = match p {
        Some(p) if !stale => tone_fg(usage::tone(p), th),
        _ => th.dim,
    };
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
    if let Some(w) = w {
        let label = nebula_core::usage_window::format(w, now, nebula_core::usage_window::local);
        if let Some(countdown) = &label.countdown {
            out.push(Span::styled(
                format!("  {} {countdown}", label.resets),
                Style::default().fg(th.dim),
            ));
        }
    }
    if let Some(w) = w.filter(|_| stale) {
        let read = if w.read_at > 0 {
            format!("  read {}", age(w.read_at * 1000, now * 1000))
        } else {
            "  read time unknown".to_string()
        };
        out.push(Span::styled(read, Style::default().fg(th.warn)));
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
    // The star on the harness's default; dim on one flagged default but
    // switched off, which no launch takes.
    let star = if row.default {
        Span::styled(" * ", Style::default().fg(th.accent))
    } else if row.default_off {
        Span::styled(" * ", Style::default().fg(th.dim))
    } else {
        Span::raw("   ")
    };
    let mut head = vec![star, Span::styled(format!(" {} ", row.label()), name_style)];
    if row.profile.is_some() {
        head.push(Span::styled(
            format!("  {}", row.harness),
            Style::default().fg(th.muted),
        ));
    }
    if row.disabled() {
        let off = if row.default_off {
            "  off (default)"
        } else {
            "  off"
        };
        head.push(Span::styled(off, Style::default().fg(th.warn)));
    }
    if let Some(problem) = &row.problem {
        head.push(Span::styled(
            format!("  {problem}"),
            Style::default().fg(th.err),
        ));
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
    // How the account is used, ahead of where its numbers came from: the
    // live sessions on it, by name.
    let used = row.sessions_text();
    let mut last = Vec::new();
    if !used.is_empty() {
        last.push(Span::styled(
            format!("   {used}"),
            Style::default().fg(th.muted),
        ));
        last.push(Span::styled(" \u{b7}", Style::default().fg(th.dim)));
    }
    last.push(Span::styled(
        if used.is_empty() {
            source
        } else {
            source.replacen("   ", " ", 1)
        },
        Style::default().fg(th.dim),
    ));
    lines.push(Line::from(last));
    lines
}

/// One button of the overlay: `[key] label`, the key in the accent (or,
/// for a destructive one, the error color), dim while it does nothing
/// for the selected row.
struct Button {
    key: &'static str,
    label: String,
    hit: AccountsHit,
    on: bool,
    danger: bool,
}

fn button(key: &'static str, label: impl Into<String>, code: KeyCode) -> Button {
    Button {
        key,
        label: label.into(),
        hit: AccountsHit::Key(code),
        on: true,
        danger: false,
    }
}

/// Lines laid out top-down inside `inner`, recording each button's and
/// row's rect as it goes; nothing below the box registers.
struct Layout {
    inner: Rect,
    lines: Vec<Line<'static>>,
    hits: Vec<(Rect, AccountsHit)>,
}

impl Layout {
    fn y(&self) -> Option<u16> {
        let at = self.lines.len() as u16;
        (at < self.inner.height).then_some(self.inner.y + at)
    }

    fn push(&mut self, line: Line<'static>) {
        self.lines.push(line);
    }

    fn hit(&mut self, x: u16, width: u16, hit: AccountsHit) {
        if let Some(y) = self.y() {
            let x = self.inner.x + x;
            let end = self.inner.x + self.inner.width;
            if x < end {
                self.hits.push((
                    Rect {
                        x,
                        y,
                        width: width.min(end - x),
                        height: 1,
                    },
                    hit,
                ));
            }
        }
    }

    /// `buttons` across as many lines as the width needs.
    fn buttons(&mut self, buttons: Vec<Button>, th: Theme) {
        let width = self.inner.width as usize;
        let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
        let mut x = 1usize;
        let mut pending: Vec<(usize, usize, AccountsHit)> = Vec::new();
        for b in buttons {
            let text_w = b.key.chars().count() + 3 + b.label.chars().count();
            if x + text_w > width && x > 1 {
                for (bx, bw, hit) in pending.drain(..) {
                    self.hit(bx as u16, bw as u16, hit);
                }
                let line = std::mem::replace(&mut spans, vec![Span::raw(" ")]);
                self.push(Line::from(line));
                x = 1;
            }
            let (key_fg, label_fg) = match (b.on, b.danger) {
                (false, _) => (th.dim, th.dim),
                (true, true) => (th.err, th.text),
                (true, false) => (th.accent, th.text),
            };
            spans.push(Span::styled(
                format!("[{}]", b.key),
                Style::default().fg(key_fg),
            ));
            spans.push(Span::styled(
                format!(" {}", b.label),
                Style::default().fg(label_fg),
            ));
            spans.push(Span::raw("  "));
            if b.on {
                pending.push((x, text_w, b.hit));
            }
            x += text_w + 2;
        }
        for (bx, bw, hit) in pending {
            self.hit(bx as u16, bw as u16, hit);
        }
        self.push(Line::from(spans));
    }
}

/// The ACCOUNT SPREAD on top: each spread a button (the one in force
/// lit), `s` stepping to the next, the failover threshold (`t`), and
/// what the spread does, in words.
fn spread_lines(l: &mut Layout, view: &AccountsView, th: Theme) {
    let now = nebula_core::accounts::Spread::parse(&view.spread);
    let lead = " Spread [s] ";
    let mut spans = vec![Span::styled(lead, Style::default().fg(th.muted))];
    let mut x = lead.chars().count();
    let mut hits = Vec::new();
    for (i, word) in nebula_core::accounts::SPREADS.iter().enumerate() {
        let label = format!(" {} ", word.replace('_', " "));
        let style = if nebula_core::accounts::Spread::parse(word) == now {
            Style::default().fg(th.on_accent).bg(th.accent)
        } else {
            Style::default().fg(th.text)
        };
        let w = label.chars().count();
        hits.push((x, w, AccountsHit::Spread(i)));
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
        x += w + 1;
    }
    let failover = now == nebula_core::accounts::Spread::Failover;
    let threshold = format!(" [t] threshold {}% ", view.threshold.trim());
    hits.push((
        x + 1,
        threshold.chars().count(),
        AccountsHit::Key(KeyCode::Char('t')),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        threshold,
        Style::default().fg(if failover { th.text } else { th.dim }),
    ));
    for (x, w, hit) in hits {
        l.hit(x as u16, w as u16, hit);
    }
    l.push(Line::from(spans));
    l.push(Line::from(Span::styled(
        format!(" {}", crate::accounts_overlay::spread_text(&view.spread)),
        Style::default().fg(th.dim),
    )));
}

/// The list's buttons: what the selected row can do.
fn list_buttons(view: &AccountsView) -> Vec<Button> {
    let row = view.selected();
    let profile = row.is_some_and(|r| r.profile.is_some());
    let usable = profile && row.is_some_and(|r| r.problem.is_none());
    let off = row.is_some_and(AccountRow::disabled);
    let mut out = vec![
        button("a", "add", KeyCode::Char('a')),
        button("\u{21b5}", "edit", KeyCode::Enter),
        button("c", "duplicate", KeyCode::Char('c')),
        button("r", "rename", KeyCode::Char('r')),
        button("x", "remove", KeyCode::Char('x')),
        button("d", "default", KeyCode::Char('d')),
        button(
            "\u{2423}",
            if off { "switch on" } else { "switch off" },
            KeyCode::Char(' '),
        ),
        button("l", "sign in", KeyCode::Char('l')),
        button("p", "use for repo", KeyCode::Char('p')),
        button("m", "move session here", KeyCode::Char('m')),
    ];
    for b in &mut out[1..] {
        b.on = match b.key {
            "\u{21b5}" | "c" | "r" | "x" => profile,
            "d" | "\u{2423}" | "p" => usable,
            "l" => row.is_some_and(|r| r.problem.is_none()),
            _ => row.is_some(),
        };
    }
    out[4].danger = true;
    // Wake-up runs Claude accounts only.
    let claude = row.is_some_and(|r| r.harness == "claude" && r.problem.is_none());
    let mut wake = button("w", "wake now", KeyCode::Char('w'));
    wake.on = claude;
    out.push(wake);
    out.push(button("Tab", "wake-up", KeyCode::Tab));
    out
}

/// The header: the two views as tabs, in the SETTINGS OVERLAY's strip,
/// the one up lit; a click on one turns to it.
fn tab_lines(l: &mut Layout, view: &AccountsView, th: Theme) {
    let tabs = [(Tab::Accounts, "Accounts"), (Tab::Wake, "Wake")];
    let active = tabs.iter().position(|(t, _)| *t == view.tab).unwrap_or(0);
    let (strip, hits) = tab_strip(
        l.inner.x,
        tabs.iter().map(|(_, name)| (*name, None)),
        active,
        false,
        th,
    );
    for ((from, to), (tab, _)) in hits.into_iter().zip(tabs) {
        l.hit(from - l.inner.x, to - from, AccountsHit::Tab(tab));
    }
    l.push(Line::from(strip));
    l.push(strip_rule(l.inner.width, th));
}

fn draw_list(l: &mut Layout, view: &AccountsView, now: i64, th: Theme) {
    tab_lines(l, view, th);
    spread_lines(l, view, th);
    for row in &view.rows {
        let banner = row.usage.as_ref().and_then(|u| {
            let w = u.window(usage::WindowKind::Session)?;
            nebula_core::usage_window::format(w, now, nebula_core::usage_window::local).banner
        });
        if let Some(text) = banner {
            l.push(Line::from(Span::styled(
                format!(" {}: {text}", row.label()),
                Style::default().fg(th.err),
            )));
        }
    }
    l.buttons(list_buttons(view), th);
    l.push(Line::from(""));
    if view.rows.is_empty() {
        l.push(Line::from(Span::styled(
            " No accounts yet: a adds one; usage shows after a session's first turn.",
            Style::default().fg(th.dim),
        )));
    }
    // Keep the cursor's account on screen: each takes five lines.
    let per = 5usize;
    let left = (l.inner.height as usize).saturating_sub(l.lines.len());
    let fit = (left / per).max(1);
    let first = view.cursor.saturating_sub(fit - 1);
    for (i, row) in view.rows.iter().enumerate().skip(first).take(fit) {
        for line in row_lines(row, i == view.cursor, now, th) {
            l.hit(0, l.inner.width, AccountsHit::Row(i));
            l.push(line);
        }
        l.push(Line::from(""));
    }
}

// ---- the Wake view ----

/// How far in the strip and the lines under an account start.
const WAKE_INDENT: usize = 4;
/// The strip's widest: a cell of a quarter hour.
const STRIP_MAX: usize = 96;
/// Room for an account's name on its line.
const WAKE_NAME: usize = 18;
/// Room for the days it fires on.
const WAKE_DAYS: usize = 13;

/// `text` onto `spans`, `x` moved on by its width.
fn put(spans: &mut Vec<Span<'static>>, x: &mut usize, text: impl Into<String>, style: Style) {
    let text = text.into();
    *x += text.chars().count();
    spans.push(Span::styled(text, style));
}

/// One cell of the day strip. The windows are the upper half of a cell
/// and the usual hours the lower, so both read on one line; a switched-off
/// account shows what it would do in the quieter tones. Ember is not used:
/// that is for what needs the user.
fn cell_span(cell: wake_tab::Cell, on: bool, th: Theme) -> Span<'static> {
    let tone = |index: usize| match (on, index % 2) {
        (true, 0) => th.ok,
        (true, _) => th.muted,
        (false, 0) => th.muted,
        (false, _) => th.dim,
    };
    match cell {
        wake_tab::Cell::Idle => Span::styled("\u{b7}", Style::default().fg(th.dim)),
        wake_tab::Cell::Usual => Span::styled("\u{2584}", Style::default().fg(th.dim)),
        wake_tab::Cell::Window { index, usual } => {
            let style = Style::default().fg(tone(index));
            Span::styled("\u{2580}", if usual { style.bg(th.dim) } else { style })
        }
        wake_tab::Cell::Now => Span::styled(
            "\u{2503}",
            Style::default().fg(th.text).add_modifier(Modifier::BOLD),
        ),
        wake_tab::Cell::Anchor => Span::styled(
            "\u{25c6}",
            Style::default().fg(th.text).add_modifier(Modifier::BOLD),
        ),
    }
}

/// An account of the Wake view, in three lines: its switch, name, hour,
/// days and next run; the day as a strip; and what the last heartbeat did
/// (or what stops the next). Every line is the row's, for a click.
fn wake_row(l: &mut Layout, tab: &WakeTab, i: usize, strip_w: usize, th: Theme) {
    let row = &tab.rows()[i];
    let selected = i == tab.cursor;
    let on = row.schedule.enabled;
    let anchor = row.schedule.anchor();
    let (now, offset) = (tab.now(), tab.offset());
    let width = l.inner.width;

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut x = 0usize;
    put(&mut spans, &mut x, " ", Style::default());
    let switch_at = x;
    let switch = format!(
        "{} {}",
        if on { "[x]" } else { "[ ]" },
        if on { "on " } else { "off" }
    );
    let switch_w = switch.chars().count();
    put(
        &mut spans,
        &mut x,
        switch,
        Style::default().fg(if on { th.ok } else { th.dim }),
    );
    put(&mut spans, &mut x, " ", Style::default());
    let name_style = if selected {
        Style::default().fg(th.on_accent).bg(th.accent)
    } else if row.disabled || !row.known {
        Style::default().fg(th.dim)
    } else {
        Style::default().fg(th.text).add_modifier(Modifier::BOLD)
    };
    put(
        &mut spans,
        &mut x,
        format!(" {:<w$} ", truncate(&row.label(), WAKE_NAME), w = WAKE_NAME),
        name_style,
    );
    put(&mut spans, &mut x, "  ", Style::default());
    let arrows_at = x;
    let hour_style = if on {
        Style::default().fg(th.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.muted)
    };
    put(
        &mut spans,
        &mut x,
        "\u{2039} ",
        Style::default().fg(th.muted),
    );
    put(
        &mut spans,
        &mut x,
        nebula_core::wake::hhmm(anchor),
        hour_style,
    );
    put(
        &mut spans,
        &mut x,
        " \u{203a}",
        Style::default().fg(th.muted),
    );
    put(&mut spans, &mut x, "  ", Style::default());
    let days = nebula_core::wake::days_text(row.schedule.day_mask());
    put(
        &mut spans,
        &mut x,
        format!("{:<w$}", truncate(&days, WAKE_DAYS), w = WAKE_DAYS),
        Style::default().fg(th.dim),
    );
    if let Some(next) = row.next_run(now, offset).filter(|_| on) {
        put(
            &mut spans,
            &mut x,
            format!("  next {}", wake_tab::day_clock(next, offset)),
            Style::default().fg(th.muted),
        );
    }
    // The most specific spots first: a click takes the first that holds it.
    l.hit(
        switch_at as u16,
        switch_w as u16,
        AccountsHit::WakeSwitch(i),
    );
    l.hit(arrows_at as u16, 2, AccountsHit::WakeAnchor(i, -1));
    l.hit(arrows_at as u16 + 7, 2, AccountsHit::WakeAnchor(i, 1));
    l.hit(0, width, AccountsHit::WakeRow(i));
    l.push(Line::from(spans));

    let minute = nebula_core::wake::LocalTime::at(now, offset).minute;
    let cells = wake_tab::strip(strip_w, &row.windows_at(anchor), row.usual, anchor, minute);
    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(WAKE_INDENT))];
    spans.extend(cells.into_iter().map(|cell| cell_span(cell, on, th)));
    l.hit(0, width, AccountsHit::WakeRow(i));
    l.push(Line::from(spans));

    // What needs the user first, then the last decision, then as much of
    // the usual hours and the suggestion as the width holds.
    let mut parts: Vec<(String, Color)> = Vec::new();
    if let Some(problem) = row.problem() {
        parts.push((problem, th.warn));
    }
    parts.push((
        match row.last_summary.as_deref().filter(|s| !s.is_empty()) {
            Some(summary) => format!("last {summary}"),
            None => "no wake-ups yet".to_string(),
        },
        th.muted,
    ));
    if let Some(usual) = row.usual {
        parts.push((format!("usual {}", usual.text()), th.dim));
    }
    if let Some(best) = row.suggested {
        parts.push((
            format!("suggested {}", nebula_core::wake::hhmm(best)),
            th.dim,
        ));
    }
    let room = (width as usize).saturating_sub(WAKE_INDENT + 1);
    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(WAKE_INDENT))];
    let mut used = 0usize;
    for (n, (text, fg)) in parts.into_iter().enumerate() {
        let sep = if n == 0 { 0 } else { 3 };
        let w = text.chars().count();
        if n > 0 && used + sep + w > room {
            continue;
        }
        if sep > 0 {
            spans.push(Span::styled(" \u{b7} ", Style::default().fg(th.dim)));
        }
        spans.push(Span::styled(text, Style::default().fg(fg)));
        used += sep + w;
    }
    l.hit(0, width, AccountsHit::WakeRow(i));
    l.push(Line::from(spans));
}

/// Under the accounts: what the wake-up does in plain words, where the
/// trigger stands, what the marks mean, and the keys as buttons.
fn wake_footer(l: &mut Layout, tab: &WakeTab, th: Theme) {
    let width = l.inner.width as usize;
    l.push(Line::from(""));
    for line in wrap_words(wake_tab::explain(), width.saturating_sub(2).max(10)) {
        l.push(Line::from(Span::styled(
            format!(" {line}"),
            Style::default().fg(th.muted),
        )));
    }
    if let Some(status) = &tab.status {
        if !status.task_line.is_empty() {
            l.push(Line::from(Span::styled(
                format!(" {}", status.task_line),
                Style::default().fg(th.dim),
            )));
        }
        // Where kete installs nothing, the line to add is the trigger.
        if let Some(cron) = status.crontab.first().filter(|_| !status.on_windows) {
            l.push(Line::from(Span::styled(
                format!(" {cron}"),
                Style::default().fg(th.dim),
            )));
        }
        if status.daemon == Some(false) {
            l.push(Line::from(Span::styled(
                " The running daemon is older: wake-ups fall back to claude -p until you run kete kill.",
                Style::default().fg(th.warn),
            )));
        }
    }
    l.push(Line::from(Span::styled(
        " \u{25c6} wake-up hour  \u{2503} now  \u{2580} wake windows  \u{2584} your usual hours",
        Style::default().fg(th.dim),
    )));
    let row = tab.selected();
    let mut space = button("Space", "on/off", KeyCode::Char(' '));
    space.on = row.is_some();
    let mut earlier = button("\u{2190}", "earlier", KeyCode::Left);
    earlier.on = row.is_some();
    let mut later = button("\u{2192}", "later", KeyCode::Right);
    later.on = row.is_some();
    let mut best = button("s", "suggested", KeyCode::Char('s'));
    best.on = row.is_some_and(|a| a.suggested.is_some());
    let mut wake = button("w", "wake now", KeyCode::Char('w'));
    wake.on = row.is_some();
    l.buttons(
        vec![
            space,
            earlier,
            later,
            best,
            wake,
            button("Tab", "accounts", KeyCode::Tab),
            button("Esc", "close", KeyCode::Esc),
        ],
        th,
    );
}

/// The Wake view: a scale for the strips, the accounts each in three
/// lines (scrolled to keep the cursor's on screen), then the footer.
fn draw_wake(l: &mut Layout, view: &AccountsView, th: Theme) {
    tab_lines(l, view, th);
    let tab = &view.wake;
    let width = l.inner.width as usize;
    let strip_w = width.saturating_sub(WAKE_INDENT + 2).clamp(1, STRIP_MAX);
    // What the footer will take, so the accounts leave room for it.
    let explained = wrap_words(wake_tab::explain(), width.saturating_sub(2).max(10)).len();
    let extra = tab.status.as_ref().map_or(0, |s| {
        usize::from(!s.task_line.is_empty())
            + usize::from(!s.on_windows && !s.crontab.is_empty())
            + usize::from(s.daemon == Some(false))
    });
    let reserve = 1 + explained + extra + 1 + 2;
    match &tab.status {
        None => {
            let (text, fg) = match &tab.error {
                Some(why) => (format!(" Couldn't read the wake-up status: {why}"), th.err),
                None => (" Reading the wake-up status\u{2026}".to_string(), th.dim),
            };
            l.push(Line::from(Span::styled(text, Style::default().fg(fg))));
        }
        Some(status) => {
            l.push(Line::from(Span::styled(
                format!("{}{}", " ".repeat(WAKE_INDENT), wake_tab::scale(strip_w)),
                Style::default().fg(th.dim),
            )));
            if status.accounts.is_empty() {
                l.push(Line::from(Span::styled(
                    " No Claude accounts yet: press a on the Accounts view to add one.",
                    Style::default().fg(th.dim),
                )));
            }
            // Each account takes three lines and a blank between.
            let left = (l.inner.height as usize).saturating_sub(l.lines.len() + reserve);
            let fit = ((left + 1) / 4).max(1);
            let first = tab.cursor.saturating_sub(fit - 1);
            for i in (first..status.accounts.len()).take(fit) {
                if i > first {
                    l.push(Line::from(""));
                }
                wake_row(l, tab, i, strip_w, th);
            }
            // A refresh that failed keeps the rows and says so.
            if let Some(why) = &tab.error {
                l.push(Line::from(Span::styled(
                    format!(" Couldn't refresh: {why}"),
                    Style::default().fg(th.warn),
                )));
            }
        }
    }
    wake_footer(l, tab, th);
}

fn draw_form(l: &mut Layout, form: &AccountForm, th: Theme) {
    l.push(Line::from(Span::styled(
        format!(" {}", form.title()),
        Style::default().fg(th.text).add_modifier(Modifier::BOLD),
    )));
    l.push(Line::from(""));
    for (i, field) in form.fields().iter().enumerate() {
        let selected = i == form.field;
        let label_style = if selected {
            Style::default().fg(th.on_accent).bg(th.accent)
        } else {
            Style::default().fg(th.muted)
        };
        let mut value = form.value(*field);
        if selected && AccountForm::is_text(*field) {
            value.push('\u{2581}');
        }
        l.hit(0, l.inner.width, AccountsHit::Field(i));
        l.push(Line::from(vec![
            Span::styled(format!(" {:<20}", field.label()), label_style),
            Span::styled(format!(" {value}"), Style::default().fg(th.text)),
        ]));
    }
    l.push(Line::from(""));
    let note = match form.kind {
        FormKind::Add if form.adopt => {
            "It runs on the CLI's current login: no home of its own, nothing to sign in."
        }
        FormKind::Add => "Its sign-in runs next, in a new terminal session.",
        FormKind::Edit { .. } if form.clear_env => {
            "Saving drops every variable it sets. Typed ones replace them instead."
        }
        FormKind::Edit { .. } => {
            "The name stays (r renames it from the list). Empty variables keep the ones it has; Del clears them."
        }
        FormKind::Duplicate { .. } => {
            "A copy with the same variables and kind of login, in a home of its own: it signs in separately."
        }
        FormKind::Rename { .. } => {
            "Its home, the repos it is the default of, and its usage reading move to the new name."
        }
    };
    l.push(Line::from(Span::styled(
        format!(" {note}"),
        Style::default().fg(th.dim),
    )));
    if let Some(error) = &form.error {
        l.push(Line::from(Span::styled(
            format!(" {error}"),
            Style::default().fg(th.err),
        )));
    }
    l.push(Line::from(""));
    let save = match form.kind {
        FormKind::Add => "add",
        FormKind::Edit { .. } => "save",
        FormKind::Duplicate { .. } => "duplicate",
        FormKind::Rename { .. } => "rename",
    };
    let mut buttons = vec![
        button("Enter", save, KeyCode::Enter),
        button("Tab", "next field", KeyCode::Tab),
    ];
    if form.can_clear_env() {
        let mut clear = button("Del", "clear variables", KeyCode::Delete);
        clear.danger = true;
        buttons.push(clear);
    }
    buttons.push(button("Esc", "back", KeyCode::Esc));
    l.buttons(buttons, th);
}

fn draw_remove(l: &mut Layout, message: &str, home: bool, th: Theme) {
    l.push(Line::from(Span::styled(
        " Remove account",
        Style::default().fg(th.err).add_modifier(Modifier::BOLD),
    )));
    l.push(Line::from(""));
    let width = (l.inner.width as usize).saturating_sub(2).max(10);
    for line in wrap_words(message, width) {
        l.push(Line::from(Span::styled(
            format!(" {line}"),
            Style::default().fg(th.text),
        )));
    }
    l.push(Line::from(""));
    let mut buttons = Vec::new();
    let mut remove = button(
        "y",
        if home {
            "remove, keep its home"
        } else {
            "remove"
        },
        KeyCode::Char('y'),
    );
    remove.danger = true;
    buttons.push(remove);
    if home {
        let mut purge = button("p", "remove and delete its home", KeyCode::Char('p'));
        purge.danger = true;
        buttons.push(purge);
    }
    buttons.push(button("n", "keep it", KeyCode::Char('n')));
    l.buttons(buttons, th);
}

/// `text` broken at spaces into lines of at most `width` columns.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn draw_use_for(l: &mut Layout, view: &AccountsView, cursor: usize, th: Theme) {
    let Some(profile) = view.selected().and_then(|r| r.profile.clone()) else {
        return;
    };
    let (name, harness) = (profile.name.trim(), profile.harness.trim());
    l.push(Line::from(Span::styled(
        format!(" Use {name} as the default {harness} account of"),
        Style::default().fg(th.text).add_modifier(Modifier::BOLD),
    )));
    l.push(Line::from(""));
    for (i, (project, repo)) in view.projects.iter().enumerate() {
        let now = view
            .project_accounts
            .get(repo)
            .and_then(|m| m.get(harness))
            .map(|n| n.trim().to_string());
        let text = match now.as_deref() {
            Some(n) if n == name => format!(" \u{2713} {project}  (Enter clears it)"),
            Some(n) => format!("   {project}  (now {n})"),
            None => format!("   {project}"),
        };
        let style = if i == cursor {
            Style::default().fg(th.on_accent).bg(th.accent)
        } else {
            Style::default().fg(th.text)
        };
        l.hit(0, l.inner.width, AccountsHit::Project(i));
        l.push(Line::from(Span::styled(text, style)));
    }
    l.push(Line::from(""));
    l.buttons(
        vec![
            button("Enter", "set / clear", KeyCode::Enter),
            button("Esc", "back", KeyCode::Esc),
        ],
        th,
    );
}

/// `line` cut to `width` columns with an ellipsis, so a long one (the
/// spread's words, a row's sessions) ends on `…` rather than at the frame.
fn fit_line(line: Line<'static>, width: u16) -> Line<'static> {
    let width = usize::from(width);
    if line.width() <= width || width == 0 {
        return line;
    }
    let mut room = width - 1;
    let mut spans = Vec::new();
    for span in line.spans {
        let w = span.width();
        if w <= room {
            room -= w;
            spans.push(span);
            continue;
        }
        let cut: String = span.content.chars().take(room).collect();
        spans.push(Span::styled(cut, span.style));
        break;
    }
    spans.push(Span::raw("…"));
    Line::from(spans).style(line.style)
}

pub(super) fn draw(f: &mut Frame, app: &mut App, mut view: AccountsView) {
    // What the Wake view's threads landed since the last frame: the loop
    // redraws when one answers, and this is where the view takes it.
    view.wake.absorb();
    let th = app.chrome.theme;
    let now = nebula_core::clock::now_ms();
    // Floor of 8 rows, but never taller than the terminal allows (a
    // plain clamp panics once the ceiling drops under the floor).
    let ceiling = f.area().height.saturating_sub(2);
    let want = match &view.mode {
        Mode::List if view.tab == Tab::Wake => view.wake.rows().len().max(1) as u16 * 4 + 14,
        Mode::List => view.rows.len() as u16 * 5 + 11,
        Mode::UseFor { .. } => view.projects.len() as u16 + 7,
        _ => 16,
    };
    let height = want.max(8).min(ceiling);
    let width = f.area().width.saturating_sub(4).min(96);
    let area = centered_rect(f.area(), width, height);
    let inner = render_modal_frame(f, area, " Accounts ", th);
    let mut l = Layout {
        inner,
        lines: Vec::new(),
        hits: Vec::new(),
    };
    match &view.mode {
        Mode::List if view.tab == Tab::Wake => draw_wake(&mut l, &view, th),
        Mode::List => draw_list(&mut l, &view, now, th),
        Mode::Form(form) => draw_form(&mut l, form, th),
        Mode::Remove { message, home, .. } => draw_remove(&mut l, message, home.is_some(), th),
        Mode::UseFor { cursor } => draw_use_for(&mut l, &view, *cursor, th),
    }
    let lines: Vec<Line> = l
        .lines
        .into_iter()
        .map(|line| fit_line(line, inner.width))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
    view.area = area;
    view.hits = l.hits;
    if let Some(Overlay::Accounts(v)) = &mut app.modals.overlay {
        *v = view;
    }
}
