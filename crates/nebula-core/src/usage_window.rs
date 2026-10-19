//! How a usage window reads to a person: the percent, when it resets in
//! the local time zone, and the time left. The one formatter every surface
//! calls (desktop header and rail, kanban cards, the terminal app, the
//! window title, the status line), so they cannot disagree.
//!
//! Times are shown in the zone `tz` names: production passes
//! [`local`], the OS zone (Pacific/Auckland: NZDT in October). UTC never
//! appears except in the ISO string meant for a tooltip.

use crate::usage::{pct_text, tone, Tone, UsageWindow, WindowKind};

/// The OS zone's offset from UTC at an instant, seconds east.
pub fn local(epoch_secs: i64) -> i32 {
    crate::clock::local_offset_secs_at(epoch_secs)
}

/// A window, ready to print.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Label {
    /// Percent used as of now (0 once the reset has passed); None unknown.
    pub percent: Option<f32>,
    /// Colour step: 0 below 70, 1 from 70, 2 from 85, 3 from 95.
    pub step: u8,
    /// `9:50 pm`; `Wed 2:10 am` on another day, `28 Oct 2:10 am` past a week.
    pub clock: Option<String>,
    /// `in 1 h 12 m`.
    pub countdown: Option<String>,
    /// When the window began, as `clock` is written.
    pub start: Option<String>,
    /// The reset as RFC 3339 in the local offset, for a tooltip.
    pub iso: Option<String>,
    /// The reset was worked out, not reported.
    pub estimated: bool,
    /// `87% · resets 9:50 pm`: the title, tray and status line string.
    pub compact: String,
    /// `87% · resets 9:50 pm · in 1 h 12 m`.
    pub long: String,
    /// `resets 9:50 pm`, `resets 9:50 pm (estimated)`; empty when unknown.
    pub resets: String,
    /// The hover text: start, reset, countdown and the ISO time.
    pub hover: String,
    /// The 95 percent banner for the 5-hour window.
    pub banner: Option<String>,
}

impl Label {
    /// The colour step as a [`Tone`].
    pub fn tone(&self) -> Tone {
        self.percent.map_or(Tone::Ok, tone)
    }
}

/// `in 1 h 12 m`, `in 45 m`, `in 2 d 4 h`; `now` when due.
pub fn countdown(left_secs: i64) -> String {
    if left_secs <= 0 {
        return "now".into();
    }
    let (d, h, m) = (
        left_secs / 86_400,
        (left_secs % 86_400) / 3600,
        (left_secs % 3600) / 60,
    );
    if d > 0 {
        format!("in {d} d {h} h")
    } else if h > 0 {
        format!("in {h} h {m} m")
    } else {
        format!("in {} m", m.max(1))
    }
}

/// Format `w` as of `now` (epoch seconds) in the zone `tz`.
pub fn format(w: &UsageWindow, now: i64, tz: impl Fn(i64) -> i32) -> Label {
    let percent = w.shown_at(now);
    let upcoming = w.resets_at.filter(|at| *at > now);
    let clock = upcoming.map(|at| clock_text(at, now, &tz));
    let estimated = w.estimated && upcoming.is_some();
    let resets = match (&clock, estimated) {
        (Some(c), true) => format!("resets {c} (estimated)"),
        (Some(c), false) => format!("resets {c}"),
        (None, _) => String::new(),
    };
    let compact = if resets.is_empty() {
        pct_text(percent)
    } else {
        format!("{} \u{b7} {resets}", pct_text(percent))
    };
    let countdown = upcoming.map(|at| countdown(at - now));
    let start = upcoming
        .map(|at| at - w.kind.nominal_secs())
        .filter(|s| *s > 0)
        .map(|s| clock_text(s, now, &tz));
    let iso = upcoming.map(|at| iso_text(at, &tz));
    let long = match &countdown {
        Some(c) => format!("{compact} \u{b7} {c}"),
        None => compact.clone(),
    };
    let hover = [
        start.as_ref().map(|s| format!("window began {s}")),
        (!resets.is_empty()).then(|| resets.clone()),
        countdown.clone(),
        iso.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");
    let banner = match (&clock, w.kind, percent) {
        (Some(c), WindowKind::Session, Some(p)) if p >= crate::usage::HOT_AT => Some(format!(
            "5-hour window resets at {c}, agents paused by the limit resume on their own after that"
        )),
        _ => None,
    };
    let step = match percent.map_or(Tone::Ok, tone) {
        Tone::Ok => 0,
        Tone::Watch => 1,
        Tone::Warn => 2,
        Tone::Hot => 3,
    };
    Label {
        percent,
        step,
        clock,
        countdown,
        start,
        iso,
        estimated,
        compact,
        long,
        resets,
        hover,
        banner,
    }
}

/// The window the header, title and status line lead with: the account's
/// session (5-hour) window, else the fullest of the rest.
pub fn headline(windows: &[UsageWindow], now: i64) -> Option<&UsageWindow> {
    windows
        .iter()
        .find(|w| w.kind == WindowKind::Session)
        .or_else(|| {
            windows.iter().max_by(|a, b| {
                a.percent_at(now)
                    .partial_cmp(&b.percent_at(now))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

struct Civil {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    weekday: usize,
    /// Local days since 1970-01-01.
    days: i64,
}

fn civil(epoch: i64, tz: &impl Fn(i64) -> i32) -> Civil {
    let local = epoch + tz(epoch) as i64;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    Civil {
        year: yoe + era * 400 + i64::from(month <= 2),
        month,
        day: doy - (153 * mp + 2) / 5 + 1,
        hour: secs / 3600,
        minute: (secs % 3600) / 60,
        second: secs % 60,
        weekday: (days + 4).rem_euclid(7) as usize,
        days,
    }
}

fn clock_text(epoch: i64, now: i64, tz: &impl Fn(i64) -> i32) -> String {
    let c = civil(epoch, tz);
    let (h12, ampm) = match c.hour {
        0 => (12, "am"),
        1..=11 => (c.hour, "am"),
        12 => (12, "pm"),
        _ => (c.hour - 12, "pm"),
    };
    let time = format!("{h12}:{:02} {ampm}", c.minute);
    match c.days - civil(now, tz).days {
        0 => time,
        1..=6 => format!("{} {time}", WEEKDAYS[c.weekday]),
        _ => format!("{} {} {time}", c.day, MONTHS[(c.month - 1) as usize]),
    }
}

fn iso_text(epoch: i64, tz: &impl Fn(i64) -> i32) -> String {
    let c = civil(epoch, tz);
    let off = tz(epoch);
    let (sign, a) = if off < 0 { ('-', -off) } else { ('+', off) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        c.year,
        c.month,
        c.day,
        c.hour,
        c.minute,
        c.second,
        a / 3600,
        (a % 3600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_usage::epoch_secs;

    fn at(s: &str) -> i64 {
        epoch_secs(s).unwrap()
    }
    const NZDT: fn(i64) -> i32 = |_| 13 * 3600;
    const NZST: fn(i64) -> i32 = |_| 12 * 3600;

    fn window(kind: WindowKind, percent: f32, resets_at: Option<i64>) -> UsageWindow {
        UsageWindow {
            kind,
            label: None,
            percent,
            resets_at,
            read_at: 1,
            estimated: false,
        }
    }

    #[test]
    fn nzdt_evening_reset_with_countdown_and_start() {
        // 9:50 pm NZDT is 08:50 UTC.
        let now = at("2026-10-19T07:38:00Z");
        let w = window(WindowKind::Session, 87.0, Some(at("2026-10-19T08:50:00Z")));
        let l = format(&w, now, NZDT);
        assert_eq!(l.compact, "87% \u{b7} resets 9:50 pm");
        assert_eq!(l.countdown.as_deref(), Some("in 1 h 12 m"));
        assert_eq!(l.start.as_deref(), Some("4:50 pm"));
        assert_eq!(l.iso.as_deref(), Some("2026-10-19T21:50:00+13:00"));
        assert_eq!(l.long, "87% \u{b7} resets 9:50 pm \u{b7} in 1 h 12 m");
        assert_eq!(l.step, 2);
        assert!(l.banner.is_none());
    }

    #[test]
    fn nzst_winter_offset() {
        let now = at("2026-06-10T00:00:00Z");
        let w = window(WindowKind::Session, 40.0, Some(at("2026-06-10T02:05:00Z")));
        let l = format(&w, now, NZST);
        assert_eq!(l.clock.as_deref(), Some("2:05 pm"));
        assert_eq!(l.iso.as_deref(), Some("2026-06-10T14:05:00+12:00"));
    }

    #[test]
    fn dst_crossover_uses_the_offset_at_the_reset() {
        // NZ summer time ended 2026-04-05 03:00 NZDT = 2026-04-04T14:00Z.
        let end = at("2026-04-04T14:00:00Z");
        let tz = move |t: i64| if t < end { 13 * 3600 } else { 12 * 3600 };
        let now = at("2026-04-04T12:00:00Z"); // 1:00 am NZDT
        let reset = at("2026-04-04T15:00:00Z"); // 3:00 am NZST, clocks fell back
        let w = window(WindowKind::Session, 10.0, Some(reset));
        let l = format(&w, now, tz);
        assert_eq!(l.clock.as_deref(), Some("3:00 am"));
        // Real elapsed time (3 h), not the wall-clock gap (2 h).
        assert_eq!(l.countdown.as_deref(), Some("in 3 h 0 m"));
        assert_eq!(l.iso.as_deref(), Some("2026-04-05T03:00:00+12:00"));
    }

    #[test]
    fn under_an_hour_and_due() {
        let now = at("2026-10-19T08:00:00Z");
        let w = window(WindowKind::Session, 50.0, Some(now + 25 * 60 + 30));
        assert_eq!(format(&w, now, NZDT).countdown.as_deref(), Some("in 25 m"));
        let soon = window(WindowKind::Session, 50.0, Some(now + 5));
        assert_eq!(
            format(&soon, now, NZDT).countdown.as_deref(),
            Some("in 1 m")
        );
        assert_eq!(countdown(0), "now");
    }

    #[test]
    fn over_a_day_names_the_weekday() {
        // Mon 19 Oct 2026 8:00 pm NZDT -> Wed 21 Oct 2:10 am NZDT.
        let now = at("2026-10-19T07:00:00Z");
        let w = window(WindowKind::Weekly, 30.0, Some(at("2026-10-20T13:10:00Z")));
        let l = format(&w, now, NZDT);
        assert_eq!(l.clock.as_deref(), Some("Wed 2:10 am"));
        assert_eq!(l.countdown.as_deref(), Some("in 1 d 6 h"));
        let far = window(WindowKind::Monthly, 30.0, Some(at("2026-11-12T11:00:00Z")));
        assert_eq!(
            format(&far, now, NZDT).clock.as_deref(),
            Some("13 Nov 12:00 am")
        );
    }

    #[test]
    fn estimated_says_so() {
        let now = at("2026-10-19T07:00:00Z");
        let mut w = window(WindowKind::Session, 12.0, Some(at("2026-10-19T12:00:00Z")));
        w.estimated = true;
        let l = format(&w, now, NZDT);
        assert!(l.estimated);
        assert_eq!(l.compact, "12% \u{b7} resets Tue 1:00 am (estimated)");
        assert!(l.hover.contains("window began 8:00 pm"));
    }

    #[test]
    fn no_reset_field_and_expired_show_the_percent_only() {
        let now = at("2026-10-19T07:00:00Z");
        let mut w = window(WindowKind::Session, 33.0, None);
        w.read_at = now - 60;
        assert_eq!(format(&w, now, NZDT).compact, "33%");
        let past = window(WindowKind::Session, 90.0, Some(now - 10));
        let l = format(&past, now, NZDT);
        assert_eq!(l.compact, "0%");
        assert!(l.countdown.is_none());
    }

    #[test]
    fn banner_only_for_the_session_window_at_95() {
        let now = at("2026-10-19T07:38:00Z");
        let reset = Some(at("2026-10-19T08:50:00Z"));
        let hot = window(WindowKind::Session, 95.0, reset);
        assert_eq!(
            format(&hot, now, NZDT).banner.as_deref(),
            Some("5-hour window resets at 9:50 pm, agents paused by the limit resume on their own after that")
        );
        assert!(format(&window(WindowKind::Session, 94.0, reset), now, NZDT)
            .banner
            .is_none());
        assert!(format(&window(WindowKind::Weekly, 99.0, reset), now, NZDT)
            .banner
            .is_none());
    }

    #[test]
    fn tone_steps_at_70_85_95() {
        assert_eq!(tone(69.9), Tone::Ok);
        assert_eq!(tone(70.0), Tone::Watch);
        assert_eq!(tone(85.0), Tone::Warn);
        assert_eq!(tone(95.0), Tone::Hot);
    }

    #[test]
    fn merge_estimates_a_reset_when_the_percent_drops() {
        use crate::usage::{AccountUsage, UsageSource};
        let acct = |percent: f32, read_at: i64| AccountUsage {
            account: "a".into(),
            harness: "claude".into(),
            windows: vec![UsageWindow {
                read_at,
                ..window(WindowKind::Session, percent, None)
            }],
            source: UsageSource::Transcript,
            at: 0,
            plan: None,
        };
        let mut a = acct(80.0, 1000);
        a.merge(acct(5.0, 5000));
        let w = &a.windows[0];
        assert!(w.estimated);
        assert_eq!(w.resets_at, Some(5000 + 5 * 3600));
        // A later higher reading keeps the estimate.
        a.merge(acct(9.0, 6000));
        assert_eq!(a.windows[0].resets_at, Some(5000 + 5 * 3600));
    }
}
