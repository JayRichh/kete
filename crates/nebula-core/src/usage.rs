//! ACCOUNT USAGE and SESSION CONTEXT: how much of each account's plan is
//! used, and how full each session's context window is.
//!
//! An ACCOUNT is a PROFILE (`profiles.rs`), or a harness's own default
//! login when a session runs under none — keyed `@<harness>`
//! ([`account_key`]), a name a profile can never take, so the two never
//! collide.
//!
//! Where the numbers come from, per harness (each degrades to unknown,
//! drawn "—", and no reading ever gates a launch):
//! - Claude: the documented statusLine JSON (`rate_limits.five_hour` /
//!   `seven_day`, `context_window`), handed over by kete's statusLine
//!   wrapper ([`claude_statusline`]); compactions from the transcript's
//!   `compact_boundary` lines, and the transcript's last assistant usage
//!   as the context fallback ([`TranscriptScan`]); the on-screen "used N%
//!   of your weekly limit" notice ([`crate::profiles::Usage::parse`]) last.
//! - Codex: the rollout's `token_count` events and `compacted` lines
//!   ([`TranscriptScan`]).
//! - Copilot: `gh api copilot_internal/user`'s quota snapshots
//!   ([`copilot_quota`]) for the account, the session's `events.jsonl`
//!   `session.usage_record` for its context.
//!
//! The account's latest reading is kept in the same file the screen
//! reading always used (`<profiles>/.usage/<key>.json`): the new fields
//! ride beside `percent` / `window`, which keep the most constrained
//! window, so a build that knows only [`crate::profiles::Usage`] still
//! reads it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::AgentId;

/// Which limit a window measures. On the wire by name: a new variant needs
/// a protocol gate (`protocol::USAGE_EVENTS_PROTOCOL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    /// The rolling session limit: Claude's five-hour, Codex's primary.
    Session,
    /// The seven-day limit.
    Weekly,
    /// A model's own weekly limit (`Opus weekly`); its label names it.
    ModelWeekly,
    /// A monthly allowance: Copilot's premium requests or chat.
    Monthly,
}

impl WindowKind {
    /// Short name for a bar's caption.
    pub fn short(self) -> &'static str {
        match self {
            WindowKind::Session => "5h",
            WindowKind::Weekly => "wk",
            WindowKind::ModelWeekly => "model wk",
            WindowKind::Monthly => "mo",
        }
    }

    /// The window's nominal length in seconds: how long a reading with no
    /// reset time can still be true.
    pub fn nominal_secs(self) -> i64 {
        match self {
            WindowKind::Session => 5 * 3600,
            WindowKind::Weekly | WindowKind::ModelWeekly => 7 * 86_400,
            WindowKind::Monthly => 30 * 86_400,
        }
    }

    /// How old a reading may get before a bar draws it as stale: a few
    /// hours for the session window, the window's own length otherwise.
    pub fn stale_after_secs(self) -> i64 {
        match self {
            WindowKind::Session => 3 * 3600,
            kind => kind.nominal_secs(),
        }
    }

    /// Does a spread weigh this window? Only the session (5h) and weekly
    /// limits, as documented: a model weekly or a monthly allowance never
    /// moves a launch to another account.
    pub fn spread_counts(self) -> bool {
        matches!(self, WindowKind::Session | WindowKind::Weekly)
    }
}

/// One measured limit. Positional on the wire (rmp arrays): fields are
/// only ever appended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub kind: WindowKind,
    /// Which model or allowance, where the kind alone doesn't say:
    /// `Opus` for a model weekly, `premium` / `chat` for Copilot.
    pub label: Option<String>,
    /// Percent used, 0–100 (more once a limit is overrun).
    pub percent: f32,
    /// Epoch seconds it resets at, when the source says.
    pub resets_at: Option<i64>,
    /// Epoch seconds this window was read; 0 = unknown (a reading kept by
    /// an older build), which counts as long expired when there is no
    /// reset time either.
    #[serde(default)]
    pub read_at: i64,
    /// `resets_at` was worked out from when the percent last dropped, not
    /// reported by the source. Appended field: older senders omit it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub estimated: bool,
}

impl UsageWindow {
    fn new(kind: WindowKind, percent: f32, resets_at: Option<i64>) -> Self {
        Self {
            kind,
            label: None,
            percent: percent.max(0.0),
            resets_at,
            read_at: 0,
            estimated: false,
        }
    }

    /// Has the window run out as of `now_secs`? Past its reset time; or,
    /// with no reset time, once its nominal length has passed since it
    /// was read (never read: at once).
    pub fn expired_at(&self, now_secs: i64) -> bool {
        match self.resets_at {
            Some(at) if at > 0 => at <= now_secs,
            _ => self.read_at <= 0 || self.read_at + self.kind.nominal_secs() <= now_secs,
        }
    }

    /// The percent as of `now_secs`: an expired window is empty.
    pub fn percent_at(&self, now_secs: i64) -> f32 {
        if self.expired_at(now_secs) {
            0.0
        } else {
            self.percent
        }
    }

    /// What a bar shows as of `now_secs`: the percent, 0 past a known
    /// reset, and unknown (None, drawn "—") once a window with no reset
    /// time has aged out: nothing says what it is now.
    pub fn shown_at(&self, now_secs: i64) -> Option<f32> {
        match self.resets_at {
            Some(at) if at > 0 => Some(self.percent_at(now_secs)),
            _ if self.expired_at(now_secs) => None,
            _ => Some(self.percent),
        }
    }

    /// Is the reading too old to trust at a glance (a bar draws it dim)?
    pub fn stale_at(&self, now_secs: i64) -> bool {
        self.read_at <= 0 || now_secs - self.read_at > self.kind.stale_after_secs()
    }
}

/// Where a reading came from. On the wire by name: a new variant needs a
/// protocol gate (`protocol::USAGE_EVENTS_PROTOCOL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Claude's statusLine JSON.
    Statusline,
    /// A Claude transcript.
    Transcript,
    /// A Codex rollout.
    Rollout,
    /// `gh api copilot_internal/user`.
    CopilotApi,
    /// A Copilot session's `events.jsonl`.
    CopilotEvents,
    /// The session's screen (Claude's usage notice).
    Screen,
}

impl UsageSource {
    pub fn label(self) -> &'static str {
        match self {
            UsageSource::Statusline => "Claude status line",
            UsageSource::Transcript => "Claude transcript",
            UsageSource::Rollout => "Codex rollout",
            UsageSource::CopilotApi => "GitHub Copilot quota",
            UsageSource::CopilotEvents => "Copilot session log",
            UsageSource::Screen => "usage notice on screen",
        }
    }
}

/// An ACCOUNT's latest plan usage. Positional on the wire (rmp arrays):
/// fields are only ever appended, never skipped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountUsage {
    /// The profile name, or `@<harness>` for a default login.
    pub account: String,
    pub harness: String,
    pub windows: Vec<UsageWindow>,
    pub source: UsageSource,
    /// Epoch ms it was read.
    pub at: i64,
    /// The plan, when the source names it (`plus`, `individual`).
    pub plan: Option<String>,
}

impl AccountUsage {
    pub fn window(&self, kind: WindowKind) -> Option<&UsageWindow> {
        self.windows.iter().find(|w| w.kind == kind)
    }

    /// Stamp every window not yet stamped as read at `now_secs`.
    pub fn stamp(&mut self, now_secs: i64) {
        for w in &mut self.windows {
            if w.read_at <= 0 {
                w.read_at = now_secs;
            }
        }
    }

    /// The most used session (5h) or weekly window as of `now_secs`: what
    /// a spread compares. Expired windows count as empty; None when the
    /// account has neither.
    pub fn worst_at(&self, now_secs: i64) -> Option<f32> {
        self.windows
            .iter()
            .filter(|w| w.kind.spread_counts())
            .map(|w| w.percent_at(now_secs))
            .fold(None, |acc: Option<f32>, p| {
                Some(acc.map_or(p, |a| a.max(p)))
            })
    }

    /// The two bars a row draws, top then bottom: the session (5h) and the
    /// weekly window, or for an account with neither (Copilot) its
    /// monthly ones.
    pub fn bars(&self) -> [Option<&UsageWindow>; 2] {
        let session = self.window(WindowKind::Session);
        let weekly = self.window(WindowKind::Weekly);
        if session.is_none() && weekly.is_none() {
            let mut monthly = self
                .windows
                .iter()
                .filter(|w| w.kind == WindowKind::Monthly);
            return [monthly.next(), monthly.next()];
        }
        [session, weekly]
    }

    /// `5h 21% · wk 3%`; "—" for a window not measured.
    pub fn summary(&self, now_secs: i64) -> String {
        let parts: Vec<String> = self
            .bars()
            .iter()
            .zip(["5h", "wk"])
            .map(|(w, default)| match w {
                Some(w) => format!(
                    "{} {}",
                    w.label.as_deref().unwrap_or(match w.kind {
                        WindowKind::Monthly => "mo",
                        _ => default,
                    }),
                    pct_text(Some(w.percent_at(now_secs)))
                ),
                None => format!("{default} \u{2014}"),
            })
            .collect();
        parts.join(" \u{b7} ")
    }

    /// Fold `newer` in: its windows replace those of the same kind (and
    /// label), the rest are kept — a Claude screen notice naming the weekly
    /// window must not wipe the five-hour one the status line read.
    pub fn merge(&mut self, newer: AccountUsage) {
        for w in newer.windows {
            match self
                .windows
                .iter_mut()
                .find(|old| old.kind == w.kind && old.label == w.label)
            {
                Some(old) => {
                    let mut w = w;
                    if w.resets_at.is_none() {
                        let read = w.read_at.max(1);
                        // The window rolled over when a reset time we were
                        // given has passed, or the percent fell by more
                        // than reading noise: it began at this reading.
                        let passed =
                            !old.estimated && old.resets_at.is_some_and(|at| at > 0 && at <= read);
                        if passed || old.percent - w.percent >= ROLLOVER_DROP {
                            w.resets_at = Some(read + w.kind.nominal_secs());
                            w.estimated = true;
                        } else if let Some(at) = old.resets_at.filter(|at| *at > read) {
                            // Still running: keep the reset we know, as
                            // reported or as estimated. A passed estimate
                            // is dropped, not carried.
                            w.resets_at = Some(at);
                            w.estimated = old.estimated;
                        }
                    }
                    *old = w;
                }
                None => self.windows.push(w),
            }
        }
        self.source = newer.source;
        self.at = newer.at;
        if newer.plan.is_some() {
            self.plan = newer.plan;
        }
    }
}

/// A session's context window. Positional on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextUsage {
    /// Percent of the window the last request filled.
    pub percent: Option<f32>,
    /// Tokens in the window after the last request.
    pub tokens: Option<u64>,
    /// The window's size in tokens.
    pub window: Option<u64>,
    /// How many times the conversation was compacted.
    pub compactions: u32,
    /// Epoch ms of the latest compaction, when known.
    pub last_compact_at: Option<i64>,
    pub source: UsageSource,
    /// Epoch ms it was read.
    pub at: i64,
}

/// One session's context, as `ServerEvent::SessionContexts` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionContext {
    pub agent: AgentId,
    pub context: ContextUsage,
}

/// The account a session under `profile` on `harness` counts against.
pub fn account_key(profile: Option<&str>, harness: &str) -> String {
    match profile.map(str::trim).filter(|p| !p.is_empty()) {
        Some(name) => name.to_string(),
        None => format!("@{}", harness.trim()),
    }
}

/// The harness of an `@<harness>` key; None for a profile's name.
pub fn default_login_harness(key: &str) -> Option<&str> {
    key.strip_prefix('@')
}

/// How alarming a percent is: steps at 70, 85 and 95.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    /// 70 and over: worth a glance.
    Watch,
    Warn,
    Hot,
}

/// How far a percent must fall between two readings, with no reset time
/// to say so, before the window counts as rolled over (not reading noise).
pub const ROLLOVER_DROP: f32 = 20.0;

pub const WATCH_AT: f32 = 70.0;
pub const WARN_AT: f32 = 85.0;
pub const HOT_AT: f32 = 95.0;

pub fn tone(percent: f32) -> Tone {
    if percent >= HOT_AT {
        Tone::Hot
    } else if percent >= WARN_AT {
        Tone::Warn
    } else if percent >= WATCH_AT {
        Tone::Watch
    } else {
        Tone::Ok
    }
}

/// `21%`, or "—" when unknown.
pub fn pct_text(percent: Option<f32>) -> String {
    match percent {
        Some(p) => format!("{}%", p.round().clamp(0.0, 999.0) as u32),
        None => "\u{2014}".into(),
    }
}

/// A `width`-cell bar of `▰` (used) and `▱` (left); all `╌` when unknown.
pub fn block_bar(percent: Option<f32>, width: usize) -> String {
    let Some(p) = percent else {
        return "\u{254c}".repeat(width);
    };
    let filled = ((p.clamp(0.0, 100.0) / 100.0) * width as f32).round() as usize;
    let filled = if p > 0.0 { filled.max(1) } else { 0 }.min(width);
    format!(
        "{}{}",
        "\u{25b0}".repeat(filled),
        "\u{25b1}".repeat(width - filled)
    )
}

/// `in 3 h 12 m`, `in 2 d 4 h`, `now` — the time left until `resets_at`.
pub fn resets_in(resets_at: i64, now_secs: i64) -> String {
    crate::usage_window::countdown(resets_at - now_secs)
}

// ---------------------------------------------------------------------
// Parsers. Each takes the source's JSON as it is and reads only the
// fields named here: anything missing or odd reads as unknown.

fn f32_at(v: &Value, path: &[&str]) -> Option<f32> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_f64().map(|n| n as f32)
}

fn u64_at(v: &Value, path: &[&str]) -> Option<u64> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_u64().or_else(|| cur.as_f64().map(|n| n as u64))
}

fn i64_at(v: &Value, path: &[&str]) -> Option<i64> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_i64().or_else(|| cur.as_f64().map(|n| n as i64))
}

/// What one statusLine JSON says.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatuslineReading {
    /// Plan windows; empty for an API-billed session (Claude sends
    /// `rate_limits` to Pro and Max subscribers only).
    pub windows: Vec<UsageWindow>,
    pub context_percent: Option<f32>,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    /// `context_window.current_usage` was null: right after `/compact`
    /// (or before the first request).
    pub usage_cleared: bool,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    /// `effort.level`: the reasoning effort the session runs at now,
    /// mid-session `/effort` changes and the model's cap included
    /// (`low` … `max`). Absent on a model with no effort parameter.
    pub effort: Option<String>,
    /// `model.id` (`claude-opus-5-5`). Read for what it says, never
    /// stored as the row's model: it drops the `[1m]` context suffix a
    /// `--model` needs (`session_model` reads the transcript instead).
    pub model_id: Option<String>,
}

/// The non-blank string at `path` in `v`.
fn str_at(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The effort level a statusLine or hook payload reports
/// (`{"effort": {"level": "high"}}`), when it reports one.
pub fn effort_level(v: &Value) -> Option<String> {
    str_at(v, &["effort", "level"])
}

/// Read Claude Code's statusLine JSON.
pub fn claude_statusline(v: &Value) -> StatuslineReading {
    let mut out = StatuslineReading {
        effort: effort_level(v),
        model_id: str_at(v, &["model", "id"]),
        ..StatuslineReading::default()
    };
    for (key, kind) in [
        ("five_hour", WindowKind::Session),
        ("seven_day", WindowKind::Weekly),
    ] {
        if let Some(p) = f32_at(v, &["rate_limits", key, "used_percentage"]) {
            out.windows.push(UsageWindow::new(
                kind,
                p,
                i64_at(v, &["rate_limits", key, "resets_at"]),
            ));
        }
    }
    // Model-specific weeklies, should a later schema add them
    // (`seven_day_opus`, `seven_day_sonnet`).
    if let Some(limits) = v.get("rate_limits").and_then(Value::as_object) {
        for (key, w) in limits {
            let Some(model) = key.strip_prefix("seven_day_") else {
                continue;
            };
            if let Some(p) = f32_at(w, &["used_percentage"]) {
                let mut window =
                    UsageWindow::new(WindowKind::ModelWeekly, p, i64_at(w, &["resets_at"]));
                window.label = Some(capitalise(model));
                out.windows.push(window);
            }
        }
    }
    let cw = v.get("context_window");
    out.context_window = cw.and_then(|c| u64_at(c, &["context_window_size"]));
    let current = cw.and_then(|c| c.get("current_usage"));
    match current {
        Some(Value::Object(_)) => {
            let c = current.unwrap();
            let tokens = [
                "input_tokens",
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
            ]
            .iter()
            .filter_map(|k| u64_at(c, &[k]))
            .sum::<u64>();
            out.context_tokens = Some(tokens);
        }
        Some(Value::Null) => out.usage_cleared = true,
        _ => {}
    }
    out.context_percent = cw
        .and_then(|c| f32_at(c, &["used_percentage"]))
        .or_else(|| match (out.context_tokens, out.context_window) {
            (Some(t), Some(w)) if w > 0 => Some(t as f32 * 100.0 / w as f32),
            _ => None,
        });
    if out.usage_cleared && out.context_percent.is_none() {
        out.context_percent = Some(0.0);
    }
    out.session_id = v
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    out.transcript_path = v
        .get("transcript_path")
        .and_then(Value::as_str)
        .map(str::to_string);
    out
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A Codex rate-limit window's kind by its length.
fn codex_kind(minutes: Option<u64>) -> WindowKind {
    match minutes {
        Some(m) if m >= 40_000 => WindowKind::Monthly,
        Some(m) if m >= 1_440 => WindowKind::Weekly,
        _ => WindowKind::Session,
    }
}

/// Tokens in a context window, and the window, when known.
pub type ContextTokens = (u64, Option<u64>);

/// What one Codex `token_count` payload says: plan windows, context, plan.
pub fn codex_token_count(
    payload: &Value,
) -> (Vec<UsageWindow>, Option<ContextTokens>, Option<String>) {
    let mut windows = Vec::new();
    let limits = payload.get("rate_limits");
    for key in ["primary", "secondary"] {
        let Some(w) = limits.and_then(|l| l.get(key)) else {
            continue;
        };
        if let Some(p) = f32_at(w, &["used_percent"]) {
            windows.push(UsageWindow::new(
                codex_kind(u64_at(w, &["window_minutes"])),
                p,
                i64_at(w, &["resets_at"]),
            ));
        }
    }
    let info = payload.get("info");
    // The last request's input, not the session's running total: that is
    // what the window holds now.
    let context = info
        .and_then(|i| u64_at(i, &["last_token_usage", "input_tokens"]))
        .map(|tokens| {
            (
                tokens,
                info.and_then(|i| u64_at(i, &["model_context_window"])),
            )
        });
    let plan = limits
        .and_then(|l| l.get("plan_type"))
        .and_then(Value::as_str)
        .map(str::to_string);
    (windows, context, plan)
}

/// Epoch seconds of `YYYY-MM-DD` at 00:00 UTC.
fn date_epoch(date: &str) -> Option<i64> {
    let mut it = date.get(..10)?.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400)
}

/// Epoch ms of an RFC 3339 UTC stamp (`2026-10-07T06:16:11.836Z`).
pub fn stamp_ms(stamp: &str) -> Option<i64> {
    let day = date_epoch(stamp)?;
    let time = stamp.get(11..19)?;
    let mut it = time.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let s: i64 = it.next()?.parse().ok()?;
    Some((day + h * 3600 + m * 60 + s) * 1000)
}

/// The windows `gh api copilot_internal/user` reports: premium requests
/// and chat, monthly. A quota that is unlimited, has none, or has an
/// entitlement of 0 is not a window — a plan without premium requests
/// would otherwise read as 100% used.
pub fn copilot_quota(v: &Value) -> (Vec<UsageWindow>, Option<String>) {
    let resets = v
        .get("quota_reset_date_utc")
        .or_else(|| v.get("quota_reset_date"))
        .and_then(Value::as_str)
        .and_then(date_epoch);
    let mut windows = Vec::new();
    for (key, label) in [("premium_interactions", "premium"), ("chat", "chat")] {
        let Some(q) = v.get("quota_snapshots").and_then(|s| s.get(key)) else {
            continue;
        };
        let unlimited = q.get("unlimited").and_then(Value::as_bool) == Some(true);
        let has = q.get("has_quota").and_then(Value::as_bool) != Some(false);
        let entitled = f32_at(q, &["entitlement"]).unwrap_or(0.0) > 0.0;
        let Some(left) = f32_at(q, &["percent_remaining"]) else {
            continue;
        };
        if unlimited || !has || !entitled {
            continue;
        }
        let mut w = UsageWindow::new(WindowKind::Monthly, 100.0 - left, resets);
        w.label = Some(label.into());
        windows.push(w);
    }
    let plan = v
        .get("copilot_plan")
        .and_then(Value::as_str)
        .map(str::to_string);
    (windows, plan)
}

/// The Claude context window a model has: 1M for a `[1m]` model, else
/// 200k (the statusLine's own default).
pub fn claude_window_for(model: Option<&str>) -> u64 {
    match model {
        Some(m) if m.to_ascii_lowercase().contains("[1m]") => 1_000_000,
        _ => 200_000,
    }
}

// ---------------------------------------------------------------------
// Session logs, read incrementally.

/// What a session log (Claude transcript, Codex rollout, Copilot
/// `events.jsonl`) has said so far, line by line. Only bytes appended
/// since the last [`TranscriptScan::feed`] are parsed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptScan {
    pub path: PathBuf,
    offset: u64,
    partial: String,
    pub compactions: u32,
    pub last_compact_at: Option<i64>,
    /// Latest context: tokens and the window, when the log says.
    pub context: Option<(u64, Option<u64>)>,
    /// Latest plan windows (Codex).
    pub windows: Vec<UsageWindow>,
    pub plan: Option<String>,
}

/// The newest bytes of a log read at once: older history is skipped on a
/// first look at a long transcript (its compactions undercount, but the
/// context reading — the last line that has one — is right).
const SCAN_MAX_BYTES: u64 = 8 * 1024 * 1024;

impl TranscriptScan {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }

    /// Read what was appended since the last call. False when the file
    /// could not be read; a file that shrank is read again from the start.
    pub fn feed(&mut self) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return false;
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            let path = std::mem::take(&mut self.path);
            *self = Self::new(path);
        }
        if len == self.offset {
            return true;
        }
        let mut start = self.offset;
        // A seek past unread bytes lands mid-line: that line is dropped.
        let jumped = len - start > SCAN_MAX_BYTES;
        if jumped {
            start = len - SCAN_MAX_BYTES;
            self.partial.clear();
        }
        if file.seek(SeekFrom::Start(start)).is_err() {
            return false;
        }
        let mut bytes = Vec::new();
        if file.take(len - start).read_to_end(&mut bytes).is_err() {
            return false;
        }
        self.offset = start + bytes.len() as u64;
        let text = String::from_utf8_lossy(&bytes);
        let mut buf = std::mem::take(&mut self.partial);
        buf.push_str(&text);
        let mut lines: Vec<&str> = buf.split('\n').collect();
        let tail = lines.pop().unwrap_or("").to_string();
        if jumped && !lines.is_empty() {
            lines.remove(0);
        }
        for line in lines {
            self.line(line);
        }
        self.partial = tail;
        true
    }

    /// Fold in one line of any of the three dialects.
    pub fn line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
        let stamp = || {
            v.get("timestamp")
                .and_then(Value::as_str)
                .and_then(stamp_ms)
        };
        match ty {
            // Claude.
            "system" if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => {
                self.compacted(stamp());
            }
            "assistant" => {
                let msg = v.get("message");
                if let Some(u) = msg.and_then(|m| m.get("usage")) {
                    let tokens: u64 = [
                        "input_tokens",
                        "cache_creation_input_tokens",
                        "cache_read_input_tokens",
                    ]
                    .iter()
                    .filter_map(|k| u64_at(u, &[k]))
                    .sum();
                    if tokens > 0 {
                        let model = msg.and_then(|m| m.get("model")).and_then(Value::as_str);
                        self.context = Some((tokens, Some(claude_window_for(model))));
                    }
                }
            }
            // Codex.
            "compacted" => self.compacted(stamp()),
            "event_msg" => {
                let payload = v.get("payload");
                if payload.and_then(|p| p.get("type")).and_then(Value::as_str)
                    == Some("token_count")
                {
                    let (windows, context, plan) = codex_token_count(payload.unwrap());
                    if !windows.is_empty() {
                        self.windows = windows;
                    }
                    if context.is_some() {
                        self.context = context;
                    }
                    if plan.is_some() {
                        self.plan = plan;
                    }
                }
            }
            // Copilot.
            "session.usage_record" => {
                let u = v.get("data").and_then(|d| d.get("usage"));
                if let Some(u) = u {
                    let tokens: u64 = ["inputTokens", "cacheReadTokens", "cacheWriteTokens"]
                        .iter()
                        .filter_map(|k| u64_at(u, &[k]))
                        .sum();
                    if tokens > 0 {
                        self.context = Some((tokens, u64_at(u, &["maxPromptTokens"])));
                    }
                }
            }
            t if t.starts_with("session.")
                && t.contains("compaction")
                && t.ends_with("complete") =>
            {
                self.compacted(stamp());
            }
            _ => {}
        }
    }

    fn compacted(&mut self, at: Option<i64>) {
        self.compactions += 1;
        if at.is_some() {
            self.last_compact_at = at;
        }
    }

    /// The scan as a session's context, when it has seen one.
    pub fn context_usage(&self, source: UsageSource, now_ms: i64) -> Option<ContextUsage> {
        let (tokens, window) = self.context?;
        Some(ContextUsage {
            percent: window
                .filter(|w| *w > 0)
                .map(|w| tokens as f32 * 100.0 / w as f32),
            tokens: Some(tokens),
            window,
            compactions: self.compactions,
            last_compact_at: self.last_compact_at,
            source,
            at: now_ms,
        })
    }
}

/// Where Codex keeps a session's rollout: the newest
/// `rollout-*-<session_id>.jsonl` under `<codex_home>/sessions`.
pub fn find_codex_rollout(codex_home: &Path, session_id: &str) -> Option<PathBuf> {
    let suffix = format!("-{}.jsonl", session_id.trim());
    let mut found: Option<PathBuf> = None;
    let mut stack = vec![codex_home.join("sessions")];
    let mut visited = 0;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > 4096 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(&suffix))
                && found.as_ref().is_none_or(|f| path > *f)
            {
                found = Some(path);
            }
        }
    }
    found
}

/// Is `path` the rollout of Codex session `session_id`
/// (`rollout-…-<session_id>.jsonl`)?
pub fn is_codex_rollout_for(path: &Path, session_id: &str) -> bool {
    let suffix = format!("-{}.jsonl", session_id.trim());
    !session_id.trim().is_empty()
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(&suffix))
}

/// A Copilot session's event log under `copilot_home`.
pub fn copilot_events_path(copilot_home: &Path, session_id: &str) -> PathBuf {
    copilot_home
        .join("session-state")
        .join(session_id.trim())
        .join("events.jsonl")
}

// ---------------------------------------------------------------------
// The kept reading.

/// The on-disk record: [`crate::profiles::Usage`]'s fields first (what an
/// older build reads), then the new ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    percent: u8,
    #[serde(default)]
    window: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resets: Option<String>,
    #[serde(default)]
    at: i64,
    #[serde(default)]
    harness: Option<String>,
    #[serde(default)]
    windows: Vec<UsageWindow>,
    #[serde(default)]
    source: Option<UsageSource>,
    #[serde(default)]
    plan: Option<String>,
}

fn legacy_window_name(w: &UsageWindow) -> String {
    match (w.kind, w.label.as_deref()) {
        (WindowKind::Session, _) => "5-hour".into(),
        (WindowKind::Weekly, _) => "weekly".into(),
        (WindowKind::ModelWeekly, Some(l)) => format!("{l} weekly"),
        (WindowKind::ModelWeekly, None) => "model weekly".into(),
        (WindowKind::Monthly, Some(l)) => format!("{l} monthly"),
        (WindowKind::Monthly, None) => "monthly".into(),
    }
}

/// A screen notice's window name as a window: `weekly`, `5-hour`,
/// `session`, `Opus weekly`.
pub fn window_from_legacy(name: &str, percent: f32, resets_at: Option<i64>) -> UsageWindow {
    let lower = name.trim().to_ascii_lowercase();
    let (kind, label) = if lower == "weekly" || lower == "7-day" {
        (WindowKind::Weekly, None)
    } else if lower.contains("hour") || lower == "session" {
        (WindowKind::Session, None)
    } else if let Some(model) = lower.strip_suffix(" weekly") {
        (WindowKind::ModelWeekly, Some(capitalise(model.trim())))
    } else if lower.contains("month") {
        (WindowKind::Monthly, None)
    } else {
        (WindowKind::Weekly, None)
    };
    UsageWindow {
        kind,
        label,
        percent,
        resets_at,
        read_at: 0,
        estimated: false,
    }
}

/// The kept reading for `key` under `root` (normally
/// [`crate::profiles::profiles_root`]).
pub fn read_account_in(root: &Path, key: &str) -> Option<AccountUsage> {
    let text = std::fs::read_to_string(crate::profiles::usage_path_in(root, key)).ok()?;
    let stored: Stored = serde_json::from_str(&text).ok()?;
    let harness = stored
        .harness
        .clone()
        .or_else(|| default_login_harness(key).map(str::to_string))
        .unwrap_or_else(|| "claude".into());
    let windows = if stored.windows.is_empty() {
        if stored.window.is_empty() {
            return None;
        }
        vec![window_from_legacy(
            &stored.window,
            stored.percent as f32,
            None,
        )]
    } else {
        stored.windows
    };
    // A window kept with no read time of its own (an older build, a
    // legacy screen reading) is as old as the file's `at`.
    let mut windows = windows;
    for w in &mut windows {
        if w.read_at <= 0 {
            w.read_at = stored.at / 1000;
        }
    }
    Some(AccountUsage {
        account: key.to_string(),
        harness,
        windows,
        source: stored.source.unwrap_or(UsageSource::Screen),
        at: stored.at,
        plan: stored.plan,
    })
}

/// Keep `usage` as its account's latest under `root`.
pub fn write_account_in(root: &Path, usage: &AccountUsage) -> std::io::Result<()> {
    let now = crate::clock::now_secs() as i64;
    let worst = usage
        .windows
        .iter()
        .max_by(|a, b| a.percent_at(now).total_cmp(&b.percent_at(now)));
    let stored = Stored {
        percent: worst.map_or(0, |w| w.percent.round().clamp(0.0, 100.0) as u8),
        window: worst.map(legacy_window_name).unwrap_or_default(),
        resets: worst.and_then(|w| w.resets_at).map(|at| resets_in(at, now)),
        at: usage.at,
        harness: Some(usage.harness.clone()),
        windows: usage.windows.clone(),
        source: Some(usage.source),
        plan: usage.plan.clone(),
    };
    let value = serde_json::to_value(&stored)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::settings::write_json(
        &crate::profiles::usage_path_in(root, &usage.account),
        &value,
    )
}

/// Every kept reading under `root`.
pub fn read_all_in(root: &Path) -> Vec<AccountUsage> {
    let Ok(entries) = std::fs::read_dir(root.join(".usage")) else {
        return Vec::new();
    };
    let mut out: Vec<AccountUsage> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let key = name.strip_suffix(".json")?;
            read_account_in(root, key)
        })
        .collect();
    out.sort_by(|a, b| a.account.cmp(&b.account));
    out
}

/// [`read_account_in`] under the profiles root.
pub fn read_account(key: &str) -> Option<AccountUsage> {
    read_account_in(&crate::profiles::profiles_root(), key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape the docs give, trimmed; no real session's values.
    fn statusline_fixture() -> Value {
        json!({
            "session_id": "abc123",
            "transcript_path": "/tmp/t.jsonl",
            "model": {"id": "claude-opus-4-7", "display_name": "Opus"},
            "context_window": {
                "total_input_tokens": 15500,
                "context_window_size": 200000,
                "used_percentage": 8,
                "current_usage": {
                    "input_tokens": 500,
                    "output_tokens": 1200,
                    "cache_creation_input_tokens": 5000,
                    "cache_read_input_tokens": 10000
                }
            },
            "rate_limits": {
                "five_hour": {"used_percentage": 23.5, "resets_at": 1738425600},
                "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600}
            }
        })
    }

    #[test]
    fn a_statusline_gives_both_windows_and_the_context() {
        let r = claude_statusline(&statusline_fixture());
        assert_eq!(r.windows.len(), 2);
        assert_eq!(r.windows[0].kind, WindowKind::Session);
        assert_eq!(r.windows[0].percent, 23.5);
        assert_eq!(r.windows[0].resets_at, Some(1738425600));
        assert_eq!(r.windows[1].kind, WindowKind::Weekly);
        assert_eq!(r.context_percent, Some(8.0));
        assert_eq!(r.context_tokens, Some(15500), "input + cache, no output");
        assert_eq!(r.context_window, Some(200000));
        assert!(!r.usage_cleared);
        assert_eq!(r.session_id.as_deref(), Some("abc123"));
        assert_eq!(r.model_id.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(r.effort, None, "no effort parameter on the model");
    }

    /// The docs' statusLine fields for the model and the live effort,
    /// and the same `effort` object a hook payload carries.
    #[test]
    fn a_statusline_names_the_live_effort_and_the_model_id() {
        let mut v = statusline_fixture();
        v["model"] = json!({"id": "claude-opus-5-5", "display_name": "Opus"});
        v["effort"] = json!({"level": "high"});
        let r = claude_statusline(&v);
        assert_eq!(r.effort.as_deref(), Some("high"));
        assert_eq!(r.model_id.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(effort_level(&json!({"effort": {"level": " "}})), None);
        assert_eq!(effort_level(&json!({"effort": "high"})), None, "odd shape");
    }

    #[test]
    fn a_statusline_without_rate_limits_or_usage_reads_as_unknown_or_cleared() {
        let r = claude_statusline(&json!({
            "context_window": {"context_window_size": 200000, "used_percentage": null, "current_usage": null}
        }));
        assert!(r.windows.is_empty(), "API billing: no plan windows");
        assert!(r.usage_cleared, "null usage right after /compact");
        assert_eq!(r.context_percent, Some(0.0));
        let r = claude_statusline(&json!({}));
        assert_eq!(r, StatuslineReading::default());
        // A percent computed when Claude leaves it out.
        let r = claude_statusline(&json!({
            "context_window": {"context_window_size": 1000, "current_usage": {"input_tokens": 250}}
        }));
        assert_eq!(r.context_percent, Some(25.0));
        // Only the five-hour window: the weekly stays absent.
        let r = claude_statusline(&json!({"rate_limits": {"five_hour": {"used_percentage": 3}}}));
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.windows[0].resets_at, None);
    }

    #[test]
    fn a_model_weekly_window_is_labelled() {
        let r = claude_statusline(&json!({"rate_limits": {
            "seven_day_opus": {"used_percentage": 60, "resets_at": 5}
        }}));
        assert_eq!(r.windows[0].kind, WindowKind::ModelWeekly);
        assert_eq!(r.windows[0].label.as_deref(), Some("Opus"));
    }

    const CODEX_LINES: &str = concat!(
        r#"{"timestamp":"2026-10-13T11:02:43.000Z","type":"session_meta","payload":{"id":"x"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-13T11:02:51.497Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":56159,"total_tokens":56533},"last_token_usage":{"input_tokens":29501,"cached_input_tokens":26496,"output_tokens":241,"total_tokens":29742},"model_context_window":475000},"rate_limits":{"limit_id":"codex","primary":{"used_percent":22.0,"window_minutes":300,"resets_at":1791891379},"secondary":{"used_percent":3.0,"window_minutes":10080,"resets_at":1792478179},"plan_type":"plus"}}}"#,
        "\n",
        r#"{"timestamp":"2026-10-13T12:00:00.000Z","type":"compacted","payload":{"message":""}}"#,
        "\n",
    );

    #[test]
    fn a_codex_rollout_gives_windows_context_and_compactions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, CODEX_LINES).unwrap();
        let mut scan = TranscriptScan::new(path.clone());
        assert!(scan.feed());
        assert_eq!(scan.windows.len(), 2);
        assert_eq!(scan.windows[0].kind, WindowKind::Session);
        assert_eq!(scan.windows[0].percent, 22.0);
        assert_eq!(scan.windows[1].kind, WindowKind::Weekly);
        assert_eq!(scan.context, Some((29501, Some(475000))), "last, not total");
        assert_eq!(scan.plan.as_deref(), Some("plus"));
        assert_eq!(scan.compactions, 1);
        assert_eq!(scan.last_compact_at, stamp_ms("2026-10-13T12:00:00Z"));
        let ctx = scan.context_usage(UsageSource::Rollout, 1).unwrap();
        assert!((ctx.percent.unwrap() - 6.21).abs() < 0.01);

        // Appended lines only; a half-written line waits for its end.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        write!(f, r#"{{"type":"compacted","#).unwrap();
        assert!(scan.feed());
        assert_eq!(scan.compactions, 1);
        writeln!(f, r#""payload":{{}}}}"#).unwrap();
        assert!(scan.feed());
        assert_eq!(scan.compactions, 2);
    }

    #[test]
    fn a_claude_transcript_counts_compactions_and_reads_the_last_usage() {
        let mut scan = TranscriptScan::default();
        scan.line(r#"{"type":"assistant","message":{"model":"claude-opus-4-7","usage":{"input_tokens":10,"cache_read_input_tokens":90000,"cache_creation_input_tokens":10000,"output_tokens":500}}}"#);
        scan.line(r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-10-07T06:16:11.836Z","compactMetadata":{"trigger":"auto","preTokens":227570}}"#);
        scan.line("not json");
        assert_eq!(scan.compactions, 1);
        assert_eq!(scan.context, Some((100010, Some(200000))));
        let ctx = scan.context_usage(UsageSource::Transcript, 0).unwrap();
        assert!((ctx.percent.unwrap() - 50.005).abs() < 0.01);
        assert_eq!(ctx.last_compact_at, stamp_ms("2026-10-07T06:16:11Z"));
        assert_eq!(claude_window_for(Some("claude-opus-4-7[1m]")), 1_000_000);
    }

    #[test]
    fn a_copilot_usage_record_gives_the_context() {
        let mut scan = TranscriptScan::default();
        scan.line(r#"{"type":"session.usage_record","data":{"usage":{"model":"m","inputTokens":14579,"outputTokens":26,"cacheReadTokens":0,"cacheWriteTokens":14576,"maxPromptTokens":272000}}}"#);
        assert_eq!(scan.context, Some((29155, Some(272000))));
    }

    #[test]
    fn copilot_quota_skips_allowances_the_plan_does_not_have() {
        let v = json!({
            "copilot_plan": "individual",
            "quota_reset_date": "2026-11-01",
            "quota_snapshots": {
                "chat": {"entitlement": 200, "percent_remaining": 99.8, "unlimited": false, "has_quota": true},
                "completions": {"entitlement": 2000, "percent_remaining": 100, "unlimited": false, "has_quota": true},
                "premium_interactions": {"entitlement": 0, "percent_remaining": 0, "unlimited": false, "has_quota": false}
            }
        });
        let (windows, plan) = copilot_quota(&v);
        assert_eq!(plan.as_deref(), Some("individual"));
        assert_eq!(
            windows.len(),
            1,
            "no premium requests is unknown, not 100% used"
        );
        assert_eq!(windows[0].label.as_deref(), Some("chat"));
        assert!((windows[0].percent - 0.2).abs() < 0.01);
        assert_eq!(windows[0].resets_at, date_epoch("2026-11-01"));
        assert_eq!(date_epoch("1970-01-02"), Some(86_400));
        let (windows, _) = copilot_quota(&json!({"quota_snapshots": {
            "premium_interactions": {"entitlement": 300, "percent_remaining": 10, "has_quota": true}
        }}));
        assert_eq!(windows[0].percent, 90.0);
        assert_eq!(windows[0].label.as_deref(), Some("premium"));
        let (windows, _) = copilot_quota(&json!({"quota_snapshots": {
            "premium_interactions": {"entitlement": 300, "percent_remaining": 10, "unlimited": true}
        }}));
        assert!(windows.is_empty());
    }

    #[test]
    fn the_kept_reading_stays_readable_by_the_old_usage_reader() {
        let dir = tempfile::tempdir().unwrap();
        let usage = AccountUsage {
            account: "work".into(),
            harness: "claude".into(),
            windows: vec![
                UsageWindow::new(WindowKind::Session, 21.0, None),
                UsageWindow::new(WindowKind::Weekly, 63.4, None),
            ],
            source: UsageSource::Statusline,
            at: 7,
            plan: None,
        };
        write_account_in(dir.path(), &usage).unwrap();
        assert_eq!(read_account_in(dir.path(), "work"), Some(usage.clone()));
        let old = crate::profiles::read_usage_in(dir.path(), "work").unwrap();
        assert_eq!((old.percent, old.window.as_str()), (63, "weekly"));

        // An old screen reading reads back as one window.
        crate::profiles::write_usage_in(
            dir.path(),
            "@claude",
            &crate::profiles::Usage::parse("You've used 91% of your weekly limit").unwrap(),
        )
        .unwrap();
        let back = read_account_in(dir.path(), "@claude").unwrap();
        assert_eq!(back.harness, "claude");
        assert_eq!(back.source, UsageSource::Screen);
        assert_eq!(
            back.windows,
            vec![UsageWindow::new(WindowKind::Weekly, 91.0, None)]
        );
        assert_eq!(read_all_in(dir.path()).len(), 2);
    }

    #[test]
    fn a_merge_keeps_windows_the_newer_reading_did_not_name() {
        let mut a = AccountUsage {
            account: "w".into(),
            harness: "claude".into(),
            windows: vec![
                UsageWindow::new(WindowKind::Session, 21.0, None),
                UsageWindow::new(WindowKind::Weekly, 50.0, None),
            ],
            source: UsageSource::Statusline,
            at: 1,
            plan: None,
        };
        a.merge(AccountUsage {
            windows: vec![window_from_legacy("weekly", 91.0, None)],
            source: UsageSource::Screen,
            at: 2,
            ..a.clone()
        });
        assert_eq!(a.window(WindowKind::Session).unwrap().percent, 21.0);
        assert_eq!(a.window(WindowKind::Weekly).unwrap().percent, 91.0);
        assert_eq!(a.at, 2);
        a.stamp(10);
        assert_eq!(a.worst_at(10), Some(91.0));
    }

    /// A window with no reset time ages out after its nominal length
    /// since it was read; one never stamped counts as expired; stale
    /// marks come sooner for the session window.
    #[test]
    fn a_window_without_a_reset_expires_after_its_nominal_length() {
        let mut w = UsageWindow::new(WindowKind::Weekly, 91.0, None);
        assert!(w.expired_at(0), "never read: expired");
        w.read_at = 1000;
        assert_eq!(w.percent_at(1000 + 7 * 86_400 - 1), 91.0);
        assert_eq!(w.percent_at(1000 + 7 * 86_400), 0.0);
        let mut s = UsageWindow::new(WindowKind::Session, 50.0, None);
        s.read_at = 1000;
        assert!(!s.stale_at(1000 + 3 * 3600));
        assert!(s.stale_at(1000 + 3 * 3600 + 1));
        assert_eq!(s.percent_at(1000 + 5 * 3600), 0.0);
        assert!(
            !w.stale_at(1000 + 86_400),
            "a weekly reading a day old is fine"
        );
        assert_eq!(s.shown_at(1000 + 5 * 3600), None, "aged out: unknown");
        assert_eq!(s.shown_at(1000), Some(50.0));
        // A known reset wins over the nominal length.
        let mut r = UsageWindow::new(WindowKind::Session, 50.0, Some(1000 + 10 * 3600));
        r.read_at = 1000;
        assert_eq!(r.percent_at(1000 + 6 * 3600), 50.0);
        assert_eq!(
            r.shown_at(1000 + 11 * 3600),
            Some(0.0),
            "past its reset: empty"
        );
    }

    /// Spreads weigh only the session and weekly windows: a model weekly
    /// at 100% never moves a launch.
    #[test]
    fn worst_at_weighs_only_session_and_weekly() {
        let mut u = AccountUsage {
            account: "w".into(),
            harness: "claude".into(),
            windows: vec![
                UsageWindow::new(WindowKind::Session, 10.0, None),
                window_from_legacy("Opus weekly", 100.0, None),
                UsageWindow::new(WindowKind::Monthly, 99.0, None),
            ],
            source: UsageSource::Screen,
            at: 0,
            plan: None,
        };
        u.stamp(5);
        assert_eq!(u.worst_at(5), Some(10.0));
        u.windows.remove(0);
        assert_eq!(u.worst_at(5), None);
    }

    /// A legacy file's windows are as old as its `at`: a two-week-old
    /// screen reading no longer counts.
    #[test]
    fn a_legacy_file_is_aged_by_its_at() {
        let dir = tempfile::tempdir().unwrap();
        let mut old =
            crate::profiles::Usage::parse("You've used 91% of your weekly limit").unwrap();
        old.at = 1_000_000; // ms
        crate::profiles::write_usage_in(dir.path(), "@claude", &old).unwrap();
        let back = read_account_in(dir.path(), "@claude").unwrap();
        assert_eq!(back.windows[0].read_at, 1000);
        assert_eq!(back.worst_at(1000 + 14 * 86_400), Some(0.0));
        assert_eq!(back.worst_at(1001), Some(91.0));
    }

    #[test]
    fn a_window_past_its_reset_reads_empty() {
        let w = UsageWindow::new(WindowKind::Session, 99.0, Some(100));
        assert_eq!(w.percent_at(99), 99.0);
        assert_eq!(w.percent_at(100), 0.0);
    }

    #[test]
    fn bars_tones_and_text() {
        assert_eq!(tone(69.9), Tone::Ok);
        assert_eq!(tone(70.0), Tone::Watch);
        assert_eq!(tone(84.9), Tone::Watch);
        assert_eq!(tone(85.0), Tone::Warn);
        assert_eq!(tone(95.0), Tone::Hot);
        assert_eq!(block_bar(Some(50.0), 4), "\u{25b0}\u{25b0}\u{25b1}\u{25b1}");
        assert_eq!(
            block_bar(Some(1.0), 4),
            "\u{25b0}\u{25b1}\u{25b1}\u{25b1}",
            "any use shows"
        );
        assert_eq!(block_bar(Some(0.0), 2), "\u{25b1}\u{25b1}");
        assert_eq!(block_bar(None, 2), "\u{254c}\u{254c}");
        assert_eq!(pct_text(None), "\u{2014}");
        assert_eq!(pct_text(Some(21.4)), "21%");
        assert_eq!(resets_in(100 + 3 * 3600 + 12 * 60, 100), "in 3 h 12 m");
        assert_eq!(resets_in(100 + 2 * 86_400 + 4 * 3600, 100), "in 2 d 4 h");
        assert_eq!(resets_in(5, 10), "now");
        assert_eq!(account_key(None, "claude"), "@claude");
        assert_eq!(account_key(Some("work"), "claude"), "work");
        assert_eq!(default_login_harness("@codex"), Some("codex"));
        let u = AccountUsage {
            account: "x".into(),
            harness: "copilot".into(),
            windows: vec![UsageWindow {
                label: Some("chat".into()),
                read_at: 1,
                estimated: false,
                ..UsageWindow::new(WindowKind::Monthly, 10.0, None)
            }],
            source: UsageSource::CopilotApi,
            at: 0,
            plan: None,
        };
        assert_eq!(u.summary(0), "chat 10% \u{b7} wk \u{2014}");
    }

    #[test]
    fn a_rollout_is_found_by_its_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let day = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("10")
            .join("14");
        std::fs::create_dir_all(&day).unwrap();
        let path = day.join("rollout-2026-10-14T00-02-43-abc-123.jsonl");
        std::fs::write(&path, "").unwrap();
        std::fs::write(day.join("rollout-2026-10-14T00-02-43-zzz.jsonl"), "").unwrap();
        assert_eq!(find_codex_rollout(dir.path(), "abc-123"), Some(path));
        assert_eq!(find_codex_rollout(dir.path(), "nope"), None);
        assert!(is_codex_rollout_for(
            &day.join("rollout-2026-10-14T00-02-43-abc-123.jsonl"),
            "abc-123"
        ));
        assert!(!is_codex_rollout_for(
            &day.join("rollout-2026-10-14T00-02-43-abc-123.jsonl"),
            "other"
        ));
        assert!(!is_codex_rollout_for(&day.join("rollout-x-.jsonl"), ""));
    }
}
