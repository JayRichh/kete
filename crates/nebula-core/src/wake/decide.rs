//! Whether a heartbeat goes out now: [`decide`], a pure function of the
//! clock, the schedule, what is known about the account, and the log.
//!
//! A heartbeat goes out only when all of these hold:
//! - the switch is on, today is one of its days, and the time has reached
//!   a SLOT (the anchor, or with keep-rolling a 5-hour step after it that
//!   falls inside the usual hours);
//! - the account is switched on in the accounts list and logged in;
//! - no heartbeat has gone out for this slot (and it hasn't failed twice);
//! - the account has no live five-hour window;
//! - with skip-idle on, the account saw use in the last N days;
//! - it is on time — or, catching up after the PC slept through the slot,
//!   still earlier than the usual start (with no usual hours known, within
//!   [`NO_HISTORY_CATCH_UP_MIN`] of the slot).

use crate::usage::UsageWindow;

use super::log::{Entry, Outcome};
use super::{hhmm, LocalTime, UsualHours, WakeSchedule};

/// A run this late after its slot is still on time: the Task Scheduler
/// starts it within a minute, kete may take a while to boot a daemon.
pub const ON_TIME_GRACE_MIN: u16 = 15;
/// With no usual hours to judge by, how late a catch-up may run.
pub const NO_HISTORY_CATCH_UP_MIN: u16 = 60;
/// Failed attempts a slot gets before it is left alone.
pub const MAX_FAILURES: usize = 2;

/// What is known about the account at decision time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountState {
    /// Switched off in the accounts list.
    pub disabled: bool,
    pub logged_in: bool,
    /// The five-hour window from the account's kept reading.
    pub session: Option<UsageWindow>,
    /// Epoch seconds of the account's last known use: its last turn, or
    /// its last usage reading.
    pub last_active_at: Option<i64>,
}

/// Everything [`decide`] weighs.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    /// Epoch seconds.
    pub now: i64,
    /// The local clock's offset from UTC at `now`, in seconds east.
    pub offset_secs: i32,
    pub schedule: &'a WakeSchedule,
    pub state: &'a AccountState,
    /// The account's recent log entries, in any order.
    pub log: &'a [Entry],
    pub usual: Option<UsualHours>,
}

/// Why no heartbeat goes out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The switch is off.
    Off,
    /// Today isn't one of its days.
    DayOff,
    /// Today's first slot is still ahead.
    NotYet {
        first: u16,
    },
    /// Switched off in the accounts list.
    Disabled,
    LoggedOut,
    /// This slot already had its heartbeat.
    AlreadyFired {
        slot: u16,
    },
    /// This slot failed [`MAX_FAILURES`] times.
    GaveUp {
        slot: u16,
    },
    /// A window is live: nothing to start.
    LiveWindow {
        resets_at: Option<i64>,
    },
    /// A keep-rolling step outside the usual hours.
    OutsideHours {
        slot: u16,
    },
    /// No use in the last `days` days.
    Idle {
        days: u32,
    },
    /// Too late to catch up the slot.
    Late {
        slot: u16,
        usual_start: Option<u16>,
    },
}

impl Skip {
    /// The log's `reason` code.
    pub fn code(&self) -> &'static str {
        match self {
            Skip::Off => "off",
            Skip::DayOff => "day_off",
            Skip::NotYet { .. } => "not_yet",
            Skip::Disabled => "disabled",
            Skip::LoggedOut => "logged_out",
            Skip::AlreadyFired { .. } => "already_fired",
            Skip::GaveUp { .. } => "gave_up",
            Skip::LiveWindow { .. } => "live_window",
            Skip::OutsideHours { .. } => "outside_hours",
            Skip::Idle { .. } => "idle",
            Skip::Late { .. } => "late",
        }
    }

    /// A line for a person, times on the clock `offset_secs` east of UTC.
    pub fn describe(&self, offset_secs: i32) -> String {
        match self {
            Skip::Off => "wake-up is off for this account".into(),
            Skip::DayOff => "not one of its days".into(),
            Skip::NotYet { first } => format!("not yet: the first wake-up is at {}", hhmm(*first)),
            Skip::Disabled => "the account is switched off".into(),
            Skip::LoggedOut => "the account isn't logged in".into(),
            Skip::AlreadyFired { slot } => format!("the {} wake-up already went out", hhmm(*slot)),
            Skip::GaveUp { slot } => format!("the {} wake-up failed twice", hhmm(*slot)),
            Skip::LiveWindow {
                resets_at: Some(at),
            } => format!(
                "a window is live until {}",
                hhmm(LocalTime::at(*at, offset_secs).minute)
            ),
            Skip::LiveWindow { resets_at: None } => "a window is live".into(),
            Skip::OutsideHours { slot } => {
                format!("{} is outside the usual hours", hhmm(*slot))
            }
            Skip::Idle { days } => format!("no use in the last {days} days"),
            Skip::Late {
                slot,
                usual_start: Some(start),
            } => format!(
                "missed {} and it's past the usual start ({})",
                hhmm(*slot),
                hhmm(*start)
            ),
            Skip::Late {
                slot,
                usual_start: None,
            } => format!("missed {} by more than an hour", hhmm(*slot)),
        }
    }
}

/// The verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Fire {
        /// The slot it fires for, epoch seconds.
        slot: i64,
        /// Its minute of the day.
        slot_min: u16,
        /// Late for the slot (the PC slept through it).
        catch_up: bool,
    },
    Skip(Skip),
}

impl Decision {
    /// The log's `reason` code.
    pub fn code(&self) -> &'static str {
        match self {
            Decision::Fire {
                catch_up: false, ..
            } => "on_time",
            Decision::Fire { catch_up: true, .. } => "catch_up",
            Decision::Skip(skip) => skip.code(),
        }
    }
}

/// Should a heartbeat go out now? See the module doc for the rules, in
/// the order they are asked.
pub fn decide(input: Inputs<'_>) -> Decision {
    let Inputs {
        now,
        offset_secs,
        schedule,
        state,
        log,
        usual,
    } = input;
    if !schedule.enabled {
        return Decision::Skip(Skip::Off);
    }
    let local = LocalTime::at(now, offset_secs);
    if !schedule.on_day(local.weekday) {
        return Decision::Skip(Skip::DayOff);
    }
    let slots = schedule.slots();
    let Some(&slot_min) = slots.iter().rev().find(|&&s| s <= local.minute) else {
        return Decision::Skip(Skip::NotYet { first: slots[0] });
    };
    let slot = local.epoch_of(slot_min, offset_secs);
    if state.disabled {
        return Decision::Skip(Skip::Disabled);
    }
    if !state.logged_in {
        return Decision::Skip(Skip::LoggedOut);
    }
    let for_slot = |outcome: Outcome| {
        log.iter()
            .filter(|e| e.decision == outcome && e.slot == Some(slot) && !e.forced)
            .count()
    };
    if for_slot(Outcome::Fired) > 0 {
        return Decision::Skip(Skip::AlreadyFired { slot: slot_min });
    }
    if for_slot(Outcome::Failed) >= MAX_FAILURES {
        return Decision::Skip(Skip::GaveUp { slot: slot_min });
    }
    if let Some(window) = state.session.as_ref().filter(|w| !w.expired_at(now)) {
        return Decision::Skip(Skip::LiveWindow {
            resets_at: window.resets_at.filter(|&at| at > 0),
        });
    }
    // The anchor always counts; a keep-rolling step only inside the usual
    // hours, when they are known.
    if slot_min != schedule.anchor() && usual.is_some_and(|u| !u.contains(slot_min)) {
        return Decision::Skip(Skip::OutsideHours { slot: slot_min });
    }
    if let Some(days) = schedule.skip_idle_days {
        let quiet = state
            .last_active_at
            .is_none_or(|at| now - at > days as i64 * 86_400);
        if quiet {
            return Decision::Skip(Skip::Idle { days });
        }
    }
    let late_by = local.minute - slot_min;
    let catch_up = late_by > ON_TIME_GRACE_MIN;
    if catch_up {
        let allowed = match usual {
            // Only a slot ahead of the usual hours, and only until they
            // start: a window started late in the day costs more than it
            // gives.
            Some(u) => {
                let ahead = |m: u16| (m + super::DAY_MIN - slot_min) % super::DAY_MIN;
                !u.contains(slot_min) && ahead(local.minute) < ahead(u.start)
            }
            None => late_by <= NO_HISTORY_CATCH_UP_MIN,
        };
        if !allowed {
            return Decision::Skip(Skip::Late {
                slot: slot_min,
                usual_start: usual.map(|u| u.start),
            });
        }
    }
    Decision::Fire {
        slot,
        slot_min,
        catch_up,
    }
}

/// The slot `now` falls in — the latest one reached today on one of the
/// schedule's days — as (epoch seconds, minute of the day). None before
/// the first, on a day off, or when the switch is off. What a skip is
/// logged against, so a slot's skip is logged once however often the
/// task runs.
pub fn current_slot(now: i64, offset_secs: i32, schedule: &WakeSchedule) -> Option<(i64, u16)> {
    let local = LocalTime::at(now, offset_secs);
    if !schedule.enabled || !schedule.on_day(local.weekday) {
        return None;
    }
    let slot_min = schedule
        .slots()
        .into_iter()
        .rev()
        .find(|&s| s <= local.minute)?;
    Some((local.epoch_of(slot_min, offset_secs), slot_min))
}

/// When the next slot comes, epoch seconds: the first one after `now` on
/// one of the schedule's days, within a week. None when it is off or has
/// no days. Usual hours leave out keep-rolling steps outside them.
pub fn next_slot(
    now: i64,
    offset_secs: i32,
    schedule: &WakeSchedule,
    usual: Option<UsualHours>,
) -> Option<i64> {
    if !schedule.enabled || schedule.day_mask() == 0 {
        return None;
    }
    let today = LocalTime::at(now, offset_secs);
    let anchor = schedule.anchor();
    let slots: Vec<u16> = schedule
        .slots()
        .into_iter()
        .filter(|&s| s == anchor || usual.is_none_or(|u| u.contains(s)))
        .collect();
    (0..8).find_map(|ahead| {
        let day = LocalTime {
            day: today.day + ahead,
            weekday: ((today.weekday as i64 + ahead) % 7) as u8,
            minute: 0,
        };
        if !schedule.on_day(day.weekday) {
            return None;
        }
        slots
            .iter()
            .map(|&s| day.epoch_of(s, offset_secs))
            .find(|&at| at > now)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::WindowKind;
    use crate::wake::{WakeMode, ALL_DAYS, WEEKDAYS};

    /// 2026-10-17 00:00 NZDT (Saturday), as epoch seconds.
    const SAT_MIDNIGHT: i64 = 1_792_148_400;
    const NZ: i32 = 13 * 3600;

    fn at(h: i64, m: i64) -> i64 {
        SAT_MIDNIGHT + h * 3600 + m * 60
    }

    fn schedule() -> WakeSchedule {
        WakeSchedule {
            enabled: true,
            anchor_min: 300,
            days: ALL_DAYS,
            ..WakeSchedule::default()
        }
    }

    fn ready() -> AccountState {
        AccountState {
            disabled: false,
            logged_in: true,
            session: None,
            last_active_at: Some(at(-10, 0)),
        }
    }

    fn window(resets_at: i64) -> UsageWindow {
        UsageWindow {
            kind: WindowKind::Session,
            label: None,
            percent: 12.0,
            resets_at: Some(resets_at),
            read_at: resets_at - 3600,
            estimated: false,
        }
    }

    fn logged(decision: Outcome, slot: i64) -> Entry {
        Entry {
            at: slot,
            account: "work".into(),
            decision,
            reason: "on_time".into(),
            mode: WakeMode::Interactive,
            resets_before: None,
            resets_after: None,
            detail: None,
            slot: Some(slot),
            forced: false,
        }
    }

    fn nine_to_five() -> Option<UsualHours> {
        Some(UsualHours {
            start: 9 * 60,
            len: 8 * 60,
        })
    }

    fn run(
        now: i64,
        s: &WakeSchedule,
        state: &AccountState,
        log: &[Entry],
        usual: Option<UsualHours>,
    ) -> Decision {
        decide(Inputs {
            now,
            offset_secs: NZ,
            schedule: s,
            state,
            log,
            usual,
        })
    }

    #[test]
    fn it_fires_at_the_anchor_on_time() {
        let d = run(at(5, 1), &schedule(), &ready(), &[], nine_to_five());
        assert_eq!(
            d,
            Decision::Fire {
                slot: at(5, 0),
                slot_min: 300,
                catch_up: false
            }
        );
        assert_eq!(d.code(), "on_time");
    }

    #[test]
    fn the_switch_the_day_and_the_hour_come_first() {
        let mut s = schedule();
        s.enabled = false;
        assert_eq!(
            run(at(5, 0), &s, &ready(), &[], None),
            Decision::Skip(Skip::Off)
        );
        let mut s = schedule();
        s.days = WEEKDAYS;
        assert_eq!(
            run(at(5, 0), &s, &ready(), &[], None),
            Decision::Skip(Skip::DayOff),
            "Saturday"
        );
        assert_eq!(
            run(at(4, 59), &schedule(), &ready(), &[], None),
            Decision::Skip(Skip::NotYet { first: 300 })
        );
    }

    #[test]
    fn a_switched_off_or_logged_out_account_is_left_alone() {
        let mut state = ready();
        state.disabled = true;
        assert_eq!(
            run(at(5, 0), &schedule(), &state, &[], None),
            Decision::Skip(Skip::Disabled)
        );
        let mut state = ready();
        state.logged_in = false;
        assert_eq!(
            run(at(5, 0), &schedule(), &state, &[], None),
            Decision::Skip(Skip::LoggedOut)
        );
    }

    #[test]
    fn one_heartbeat_per_slot_and_two_failures_at_most() {
        let slot = at(5, 0);
        let fired = [logged(Outcome::Fired, slot)];
        assert_eq!(
            run(at(5, 10), &schedule(), &ready(), &fired, None),
            Decision::Skip(Skip::AlreadyFired { slot: 300 })
        );
        // Yesterday's doesn't count, nor a forced one.
        let other = [logged(Outcome::Fired, slot - 86_400)];
        assert!(matches!(
            run(at(5, 10), &schedule(), &ready(), &other, None),
            Decision::Fire { .. }
        ));
        let mut forced = logged(Outcome::Fired, slot);
        forced.forced = true;
        assert!(matches!(
            run(at(5, 10), &schedule(), &ready(), &[forced], None),
            Decision::Fire { .. }
        ));
        let one = [logged(Outcome::Failed, slot)];
        assert!(matches!(
            run(at(5, 10), &schedule(), &ready(), &one, None),
            Decision::Fire { .. }
        ));
        let two = [logged(Outcome::Failed, slot), logged(Outcome::Failed, slot)];
        assert_eq!(
            run(at(5, 10), &schedule(), &ready(), &two, None),
            Decision::Skip(Skip::GaveUp { slot: 300 })
        );
    }

    #[test]
    fn a_live_window_means_nothing_to_start() {
        let mut state = ready();
        state.session = Some(window(at(7, 0)));
        let d = run(at(5, 0), &schedule(), &state, &[], None);
        assert_eq!(
            d,
            Decision::Skip(Skip::LiveWindow {
                resets_at: Some(at(7, 0))
            })
        );
        let Decision::Skip(skip) = d else {
            unreachable!()
        };
        assert_eq!(skip.describe(NZ), "a window is live until 07:00");
        // One that reset already is no obstacle.
        state.session = Some(window(at(4, 0)));
        assert!(matches!(
            run(at(5, 0), &schedule(), &state, &[], None),
            Decision::Fire { .. }
        ));
        // Nor one with no reset time read long ago.
        let mut old = window(0);
        old.resets_at = None;
        old.read_at = at(-6, 0);
        state.session = Some(old);
        assert!(matches!(
            run(at(5, 0), &schedule(), &state, &[], None),
            Decision::Fire { .. }
        ));
    }

    #[test]
    fn keep_rolling_steps_fire_inside_the_usual_hours_only() {
        let mut s = schedule();
        s.keep_rolling = true;
        // 10:00 and 15:00 are in 9–5, 20:00 isn't.
        assert_eq!(
            run(at(10, 2), &s, &ready(), &[], nine_to_five()),
            Decision::Fire {
                slot: at(10, 0),
                slot_min: 600,
                catch_up: false
            }
        );
        assert_eq!(
            run(at(20, 0), &s, &ready(), &[], nine_to_five()),
            Decision::Skip(Skip::OutsideHours { slot: 1200 })
        );
        // Without usual hours every step counts.
        assert!(matches!(
            run(at(20, 0), &s, &ready(), &[], None),
            Decision::Fire { slot_min: 1200, .. }
        ));
        // Without keep-rolling, 10:00 is just late for 05:00.
        assert!(matches!(
            run(at(10, 2), &schedule(), &ready(), &[], nine_to_five()),
            Decision::Skip(Skip::Late { slot: 300, .. })
        ));
    }

    #[test]
    fn an_idle_account_is_skipped_when_asked() {
        let mut s = schedule();
        s.skip_idle_days = Some(3);
        let mut state = ready();
        state.last_active_at = Some(at(5, 0) - 4 * 86_400);
        assert_eq!(
            run(at(5, 0), &s, &state, &[], None),
            Decision::Skip(Skip::Idle { days: 3 })
        );
        state.last_active_at = None;
        assert_eq!(
            run(at(5, 0), &s, &state, &[], None),
            Decision::Skip(Skip::Idle { days: 3 }),
            "never used"
        );
        state.last_active_at = Some(at(5, 0) - 2 * 86_400);
        assert!(matches!(
            run(at(5, 0), &s, &state, &[], None),
            Decision::Fire { .. }
        ));
    }

    #[test]
    fn a_catch_up_goes_out_only_before_the_usual_start() {
        // The PC slept through 05:00 and wakes at 08:30: still before 09:00.
        assert_eq!(
            run(at(8, 30), &schedule(), &ready(), &[], nine_to_five()),
            Decision::Fire {
                slot: at(5, 0),
                slot_min: 300,
                catch_up: true
            }
        );
        let late = run(at(9, 30), &schedule(), &ready(), &[], nine_to_five());
        assert_eq!(
            late,
            Decision::Skip(Skip::Late {
                slot: 300,
                usual_start: Some(540)
            })
        );
        let Decision::Skip(skip) = late else {
            unreachable!()
        };
        assert_eq!(
            skip.describe(NZ),
            "missed 05:00 and it's past the usual start (09:00)"
        );
        // Within the grace it's on time whatever the hours.
        assert!(matches!(
            run(at(5, 15), &schedule(), &ready(), &[], nine_to_five()),
            Decision::Fire {
                catch_up: false,
                ..
            }
        ));
        // No usual hours: an hour's grace.
        assert!(matches!(
            run(at(6, 0), &schedule(), &ready(), &[], None),
            Decision::Fire { catch_up: true, .. }
        ));
        assert!(matches!(
            run(at(6, 1), &schedule(), &ready(), &[], None),
            Decision::Skip(Skip::Late {
                usual_start: None,
                ..
            })
        ));
    }

    #[test]
    fn an_anchor_inside_the_usual_hours_never_catches_up() {
        let mut s = schedule();
        s.anchor_min = 600;
        assert!(matches!(
            run(at(10, 5), &s, &ready(), &[], nine_to_five()),
            Decision::Fire {
                catch_up: false,
                ..
            }
        ));
        assert!(matches!(
            run(at(11, 0), &s, &ready(), &[], nine_to_five()),
            Decision::Skip(Skip::Late { .. })
        ));
    }

    #[test]
    fn a_night_owl_catches_up_until_the_evening() {
        let owl = Some(UsualHours {
            start: 20 * 60,
            len: 7 * 60,
        });
        let mut s = schedule();
        s.anchor_min = 16 * 60;
        assert!(matches!(
            run(at(19, 0), &s, &ready(), &[], owl),
            Decision::Fire { catch_up: true, .. }
        ));
        assert!(matches!(
            run(at(21, 0), &s, &ready(), &[], owl),
            Decision::Skip(Skip::Late { .. })
        ));
    }

    #[test]
    fn local_time_follows_the_offset_not_utc() {
        // 05:00 NZDT is 16:00 UTC the day before: the same instant read
        // with a zero offset is 16:00, past a 05:00 anchor by hours.
        let now = at(5, 0);
        let d = decide(Inputs {
            now,
            offset_secs: 0,
            schedule: &schedule(),
            state: &ready(),
            log: &[],
            usual: nine_to_five(),
        });
        assert!(matches!(d, Decision::Skip(Skip::Late { .. })), "{d:?}");
        // West of UTC: 05:00 in Los Angeles (-7h).
        let la = -7 * 3600;
        let five_la = SAT_MIDNIGHT + 13 * 3600 + 7 * 3600 + 5 * 3600 - 86_400;
        assert_eq!(LocalTime::at(five_la, la).minute, 300);
        assert!(matches!(
            decide(Inputs {
                now: five_la,
                offset_secs: la,
                schedule: &schedule(),
                state: &ready(),
                log: &[],
                usual: None,
            }),
            Decision::Fire { .. }
        ));
    }

    #[test]
    fn the_next_slot_skips_days_off_and_steps_outside_the_hours() {
        let mut s = schedule();
        assert_eq!(next_slot(at(4, 0), NZ, &s, None), Some(at(5, 0)));
        assert_eq!(
            next_slot(at(6, 0), NZ, &s, None),
            Some(at(5, 0) + 86_400),
            "tomorrow"
        );
        s.days = WEEKDAYS;
        assert_eq!(
            next_slot(at(6, 0), NZ, &s, None),
            Some(at(5, 0) + 2 * 86_400),
            "Saturday to Monday"
        );
        s.days = ALL_DAYS;
        s.keep_rolling = true;
        assert_eq!(next_slot(at(6, 0), NZ, &s, None), Some(at(10, 0)));
        assert_eq!(
            next_slot(at(16, 0), NZ, &s, nine_to_five()),
            Some(at(5, 0) + 86_400),
            "20:00 is outside 9–5"
        );
        assert_eq!(next_slot(at(16, 0), NZ, &s, None), Some(at(20, 0)));
        s.enabled = false;
        assert_eq!(next_slot(at(4, 0), NZ, &s, None), None);
    }

    #[test]
    fn the_current_slot_is_the_latest_reached_today() {
        let mut s = schedule();
        assert_eq!(current_slot(at(4, 59), NZ, &s), None);
        assert_eq!(current_slot(at(5, 0), NZ, &s), Some((at(5, 0), 300)));
        assert_eq!(current_slot(at(23, 0), NZ, &s), Some((at(5, 0), 300)));
        s.keep_rolling = true;
        assert_eq!(current_slot(at(11, 0), NZ, &s), Some((at(10, 0), 600)));
        s.days = WEEKDAYS;
        assert_eq!(current_slot(at(11, 0), NZ, &s), None, "Saturday");
    }
}
