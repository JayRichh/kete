//! STOP NOTES: why a Claude session's last turn stopped short of a normal
//! end, kept beside its status rather than in it.
//!
//! Claude Code fires `StopFailure` instead of `Stop` when a turn ends on an
//! API error (`error`: `rate_limit`, `authentication_failed`, `overloaded`,
//! …), and since 2.1.234 it waits out a claude.ai usage limit in the open
//! session and continues on its own at the reset (the
//! `quota_auto_resume_*` notifications). A status alone cannot say "still
//! working, resumes 3:45pm" or "needs you: logged out", and a new
//! `AgentStatus` variant would break every older client, so the daemon
//! keeps a note per session in memory and pushes it to 62+ peers as
//! `ServerEvent::SessionStops` (`protocol::LIMITS_PROTOCOL`).
//!
//! The note's `reason` is free text on the wire: Claude's own error value,
//! or the notification behind it. A reason this build has never heard of
//! reads as an API error ([`StopTone::Error`]), never as a decode failure,
//! so Claude adding an error kind needs no protocol gate.

use serde::{Deserialize, Serialize};

use crate::ids::AgentId;

/// Claude's `StopFailure` error for a usage limit.
pub const RATE_LIMIT: &str = "rate_limit";
/// Claude's `StopFailure` error for a failed login.
pub const AUTHENTICATION_FAILED: &str = "authentication_failed";
/// The usage limit reset while the computer slept: Claude waits for Enter.
pub const QUOTA_RESUME_STALE: &str = "quota_auto_resume_stale";
/// Claude ended a usage-limit wait without continuing the task.
pub const QUOTA_RESUME_DISABLED: &str = "quota_auto_resume_disabled";

/// How far off a reset may be and Claude still waits for it on its own
/// (`interactive-mode#wait-for-a-usage-limit-to-reset`); further out, it
/// opens its usage-limit menu instead.
pub const AUTO_WAIT_MAX_SECS: i64 = 24 * 3600;

/// One session's note, as `ServerEvent::SessionStops` lists it: every
/// known one after the Snapshot, then one at a time as they change.
/// `note: None` clears the session's note. Positional on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStop {
    pub agent: AgentId,
    pub note: Option<StopNote>,
}

/// Why the session's last turn stopped short. Positional on the wire
/// (rmp arrays): fields are only ever appended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopNote {
    /// Claude's `StopFailure` error (`rate_limit`, `billing_error`, …) or
    /// the notification behind the note ([`QUOTA_RESUME_STALE`],
    /// [`QUOTA_RESUME_DISABLED`]).
    pub reason: String,
    /// Claude is waiting out a usage limit and will continue on its own:
    /// the row stays running.
    pub waiting: bool,
    /// The reset time as Claude printed it (`3:45pm`), already in local
    /// time: read off the session's screen.
    pub resumes: Option<String>,
    /// Epoch seconds the account's limit resets, from its last plan
    /// reading: the fallback when the screen said nothing.
    pub resets_at: Option<i64>,
    /// Claude's own words, for a tooltip: `error_details`, or the error
    /// line it showed.
    pub detail: Option<String>,
    /// Epoch ms it was noted.
    pub at: i64,
}

/// How a note reads: the badge's colour in both apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopTone {
    /// Waiting out a limit; the session carries on by itself. Never ember.
    Waiting,
    /// The user has to act (log in, billing, press Enter, pick an option).
    NeedsYou,
    /// The turn ended on an error; nothing continues, nobody need act.
    Error,
}

/// The class of a `StopFailure` error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopClass {
    /// A usage limit: Claude may wait it out on its own.
    Limit,
    /// The account or its credentials: only the user can fix it.
    NeedsYou,
    /// Anything else: the turn is over.
    Error,
}

/// The class of `reason`, a `StopFailure` error or a note's reason.
pub fn class(reason: &str) -> StopClass {
    match reason {
        RATE_LIMIT => StopClass::Limit,
        AUTHENTICATION_FAILED
        | "billing_error"
        | "account_on_hold"
        | "oauth_org_not_allowed"
        | "cloud_credential_error"
        | "model_not_found"
        | QUOTA_RESUME_STALE
        | QUOTA_RESUME_DISABLED => StopClass::NeedsYou,
        _ => StopClass::Error,
    }
}

/// The short words for `reason` on a card ("logged out", "API error").
/// A usage limit's own words depend on the wait: see [`StopNote::label`].
pub fn reason_label(reason: &str) -> &'static str {
    match reason {
        RATE_LIMIT => "usage limit",
        AUTHENTICATION_FAILED => "logged out",
        "billing_error" => "billing",
        "account_on_hold" => "account on hold",
        "oauth_org_not_allowed" => "org not allowed",
        "cloud_credential_error" => "credentials",
        "model_not_found" => "model not found",
        "overloaded" => "overloaded",
        "max_output_tokens" => "output limit",
        "invalid_request" => "bad request",
        QUOTA_RESUME_STALE => "limit reset",
        QUOTA_RESUME_DISABLED => "limit wait ended",
        _ => "API error",
    }
}

impl StopNote {
    /// The note's words for a card or a pane header: `resumes 3:45pm`
    /// (Claude's own clock text), `resumes in 2 h 5 m` (the account's reset,
    /// when the screen said nothing), or the reason's words.
    pub fn label(&self, now_secs: i64) -> String {
        if self.reason == RATE_LIMIT && self.waiting {
            if let Some(text) = &self.resumes {
                return format!("resumes {text}");
            }
            if let Some(at) = self.resets_at.filter(|at| *at > now_secs) {
                return format!("resumes {}", crate::usage::resets_in(at, now_secs));
            }
            return "waiting on limit".into();
        }
        if self.reason == RATE_LIMIT && self.menu() {
            return "weekly limit".into();
        }
        reason_label(&self.reason).into()
    }

    /// How the note reads. A limit Claude is not waiting out reads as
    /// needing you when its reset was too far off for Claude to wait (its
    /// usage-limit menu is open), else as the error it was.
    pub fn tone(&self) -> StopTone {
        match class(&self.reason) {
            StopClass::Limit if self.waiting => StopTone::Waiting,
            StopClass::Limit if self.menu() => StopTone::NeedsYou,
            StopClass::Limit | StopClass::Error => StopTone::Error,
            StopClass::NeedsYou => StopTone::NeedsYou,
        }
    }

    /// A limit whose reset was beyond [`AUTO_WAIT_MAX_SECS`] when it was
    /// noted: Claude opened its usage-limit menu instead of waiting.
    fn menu(&self) -> bool {
        !self.waiting
            && self
                .resets_at
                .is_some_and(|at| at - self.at / 1000 > AUTO_WAIT_MAX_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(reason: &str) -> StopNote {
        StopNote {
            reason: reason.into(),
            waiting: false,
            resumes: None,
            resets_at: None,
            detail: None,
            at: 1_000_000_000,
        }
    }

    #[test]
    fn a_waited_limit_says_when_it_resumes_and_is_never_ember() {
        let now = 1_000_000;
        let mut n = StopNote {
            waiting: true,
            ..note(RATE_LIMIT)
        };
        assert_eq!(n.label(now), "waiting on limit");
        n.resets_at = Some(now + 2 * 3600 + 5 * 60);
        assert_eq!(n.label(now), "resumes in 2 h 5 m");
        n.resumes = Some("3:45pm".into());
        assert_eq!(n.label(now), "resumes 3:45pm", "the screen's words win");
        assert_eq!(n.tone(), StopTone::Waiting);
    }

    #[test]
    fn a_limit_nobody_waits_out_is_an_error_or_the_menu() {
        let mut n = note(RATE_LIMIT);
        assert_eq!(
            (n.label(0), n.tone()),
            ("usage limit".into(), StopTone::Error)
        );
        // Noted at 1_000_000 s with a reset four days on: Claude's menu.
        n.resets_at = Some(1_000_000 + 4 * 86_400);
        assert_eq!(
            (n.label(0), n.tone()),
            ("weekly limit".into(), StopTone::NeedsYou)
        );
        n.resets_at = Some(1_000_000 + 3600);
        assert_eq!(n.tone(), StopTone::Error, "within a day Claude would wait");
    }

    #[test]
    fn every_documented_error_has_words_and_a_tone() {
        let needs_you = [
            "authentication_failed",
            "billing_error",
            "account_on_hold",
            "oauth_org_not_allowed",
            "cloud_credential_error",
            "model_not_found",
            QUOTA_RESUME_STALE,
            QUOTA_RESUME_DISABLED,
        ];
        let errors = [
            "overloaded",
            "invalid_request",
            "server_error",
            "max_output_tokens",
            "unknown",
            "a_kind_claude_adds_later",
        ];
        for reason in needs_you {
            assert_eq!(note(reason).tone(), StopTone::NeedsYou, "{reason}");
        }
        for reason in errors {
            assert_eq!(note(reason).tone(), StopTone::Error, "{reason}");
        }
        assert_eq!(note("authentication_failed").label(0), "logged out");
        assert_eq!(note("server_error").label(0), "API error");
        assert_eq!(note("a_kind_claude_adds_later").label(0), "API error");
        assert_eq!(note(QUOTA_RESUME_STALE).label(0), "limit reset");
    }
}
